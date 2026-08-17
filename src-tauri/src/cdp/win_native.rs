//! Windows-native process kill + Store app activation (no PowerShell).
//!
//! - Enumerate host PIDs via Toolhelp32 + QueryFullProcessImageNameW
//! - Terminate with OpenProcess + TerminateProcess (tree via children snap)
//! - Activate packaged apps via IApplicationActivationManager COM
//! - Resolve OpenAI Store packages via AppModel package family APIs + registry

#![cfg(windows)]

use super::host::append_diag;
use crate::engine::EngineError;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::time::{Duration, Instant};

// ── kernel32 / ole32 / advapi32 FFI ──────────────────────────────────────────

#[repr(C)]
struct ProcessEntry32W {
    dw_size: u32,
    cnt_usage: u32,
    th32_process_id: u32,
    th32_default_heap_id: usize,
    th32_module_id: u32,
    cnt_threads: u32,
    th32_parent_process_id: u32,
    pc_pri_class_base: i32,
    dw_flags: u32,
    sz_exe_file: [u16; 260],
}

const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
const PROCESS_TERMINATE: u32 = 0x0001;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const PROCESS_VM_READ: u32 = 0x0010;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 0x0000_0102;
const COINIT_APARTMENTTHREADED: u32 = 0x2;
const CLSCTX_INPROC_SERVER: u32 = 0x1;

#[link(name = "kernel32")]
extern "system" {
    fn CreateToolhelp32Snapshot(dw_flags: u32, th32_process_id: u32) -> isize;
    fn Process32FirstW(h_snapshot: isize, lppe: *mut ProcessEntry32W) -> i32;
    fn Process32NextW(h_snapshot: isize, lppe: *mut ProcessEntry32W) -> i32;
    fn CloseHandle(h: isize) -> i32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
    fn TerminateProcess(h: isize, exit_code: u32) -> i32;
    fn QueryFullProcessImageNameW(
        h: isize,
        flags: u32,
        buf: *mut u16,
        size: *mut u32,
    ) -> i32;
    fn WaitForSingleObject(h: isize, ms: u32) -> u32;
    fn GetCurrentProcessId() -> u32;
}

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(pv: *mut core::ffi::c_void, flags: u32) -> i32;
    fn CoUninitialize();
    fn CoCreateInstance(
        clsid: *const Guid,
        outer: *mut core::ffi::c_void,
        ctx: u32,
        iid: *const Guid,
        ppv: *mut *mut core::ffi::c_void,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetPackagesByPackageFamily(
        package_family_name: *const u16,
        count: *mut u32,
        package_full_names: *mut *mut u16,
        buffer_length: *mut u32,
        buffer: *mut u16,
    ) -> i32;
}

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

// CLSID_ApplicationActivationManager = 45BA127D-10A8-46EA-8AB7-56EA9078943C
const CLSID_APP_ACTIVATION: Guid = Guid {
    data1: 0x45BA_127D,
    data2: 0x10A8,
    data3: 0x46EA,
    data4: [0x8A, 0xB7, 0x56, 0xEA, 0x90, 0x78, 0x94, 0x3C],
};

// IID_IApplicationActivationManager = 2e941141-7f97-4756-ba1d-9decde894a3d
const IID_IAPP_ACTIVATION: Guid = Guid {
    data1: 0x2e94_1141,
    data2: 0x7f97,
    data3: 0x4756,
    data4: [0xba, 0x1d, 0x9d, 0xec, 0xde, 0x89, 0x4a, 0x3d],
};

/// IApplicationActivationManager vtable (first 3 IUnknown + ActivateApplication).
#[repr(C)]
struct IApplicationActivationManagerVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut IApplicationActivationManager) -> u32,
    activate_application: unsafe extern "system" fn(
        *mut IApplicationActivationManager,
        *const u16,
        *const u16,
        u32,
        *mut u32,
    ) -> i32,
}

#[repr(C)]
struct IApplicationActivationManager {
    lp_vtbl: *const IApplicationActivationManagerVtbl,
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn is_host_image_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "chatgpt.exe" || n == "codex.exe"
}

/// Path filter: only OpenAI / ChatGPT / Codex install trees (not random ChatGPT.exe).
fn path_is_openai_host(path: &str) -> bool {
    let p = path.replace('/', "\\").to_ascii_lowercase();
    if p.is_empty() {
        // Missing path (permission) — still allow if image name matched; caller decides.
        return true;
    }
    p.contains("\\windowsapps\\")
        || p.contains("openai.codex")
        || p.contains("openai.chatgpt")
        || p.contains("\\programs\\chatgpt\\")
        || p.contains("\\programs\\codex\\")
        || p.contains("\\openai\\chatgpt\\")
        || p.contains("\\openai\\codex\\")
        || p.ends_with("\\chatgpt.exe")
        || p.ends_with("\\codex.exe")
}

fn process_image_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            0,
            pid,
        );
        if h == 0 || h == -1 {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut size);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        Some(wide_to_string(&buf[..size as usize]))
    }
}

/// Host PID + full image path (when query succeeds).
#[derive(Debug, Clone)]
pub struct HostProcess {
    pub pid: u32,
    pub image_path: String,
}

/// Main host processes (ChatGPT/Codex) with OpenAI-path filter.
pub fn find_host_processes_toolhelp() -> Vec<HostProcess> {
    let mut by_pid: std::collections::BTreeMap<u32, HostProcess> = std::collections::BTreeMap::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == 0 || snap == -1 {
            return Vec::new();
        }
        let mut entry = ProcessEntry32W {
            dw_size: std::mem::size_of::<ProcessEntry32W>() as u32,
            cnt_usage: 0,
            th32_process_id: 0,
            th32_default_heap_id: 0,
            th32_module_id: 0,
            cnt_threads: 0,
            th32_parent_process_id: 0,
            pc_pri_class_base: 0,
            dw_flags: 0,
            sz_exe_file: [0; 260],
        };
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let name = wide_to_string(&entry.sz_exe_file);
                let pid = entry.th32_process_id;
                if is_host_image_name(&name) && pid > 0 {
                    let path = process_image_path(pid).unwrap_or_default();
                    if path_is_openai_host(&path) || path.is_empty() {
                        by_pid.entry(pid).or_insert(HostProcess {
                            pid,
                            image_path: path,
                        });
                    }
                }
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    by_pid.into_values().collect()
}

/// Main host PIDs (ChatGPT/Codex) with OpenAI-path filter. Fast Toolhelp path.
pub fn find_host_pids_toolhelp() -> Vec<u32> {
    find_host_processes_toolhelp()
        .into_iter()
        .map(|p| p.pid)
        .collect()
}

/// How a process command line relates to `--remote-debugging-port`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CdpArgStatus {
    /// Flag present as a real process argument.
    Forwarded,
    /// Flag was absorbed into a `codex://…path=` navigation URL (owl / protocol redirect).
    ProtocolRedirected,
    /// Readable command line without the debug flag.
    NotForwarded,
    /// Command line could not be read (ACL / architecture).
    Uninspectable,
}

/// Classify CDP argument forwarding from a process command line (unit-testable).
pub fn classify_cdp_argument_status(command_line: &str, port: u16) -> CdpArgStatus {
    let trimmed = command_line.trim();
    if trimmed.is_empty() {
        return CdpArgStatus::Uninspectable;
    }
    let flag = format!("--remote-debugging-port={port}");
    let flag_sp = format!("--remote-debugging-port {port}");
    let lower = trimmed.to_ascii_lowercase();
    let flag_l = flag.to_ascii_lowercase();
    let flag_sp_l = flag_sp.to_ascii_lowercase();

    // Owl / protocol path: `codex://…` (or chatgpt://) may embed the flag after path=
    let mut saw_protocol = false;
    for token in trimmed.split_whitespace() {
        let t = token.trim_matches('"');
        let tl = t.to_ascii_lowercase();
        if tl.starts_with("codex://") || tl.starts_with("chatgpt://") {
            saw_protocol = true;
            if tl.contains(&flag_l) || tl.contains(&flag_sp_l) || tl.contains("remote-debugging-port")
            {
                return CdpArgStatus::ProtocolRedirected;
            }
        }
    }
    // Flag appears outside protocol URLs
    if lower.contains(&flag_l) || lower.contains(&flag_sp_l) {
        // If only inside a protocol URL we already returned; plain flag → forwarded.
        if !saw_protocol {
            return CdpArgStatus::Forwarded;
        }
        // Mixed: protocol present but flag also as real arg — still count forwarded if
        // a non-protocol token carries the flag.
        for token in trimmed.split_whitespace() {
            let t = token.trim_matches('"');
            let tl = t.to_ascii_lowercase();
            if tl.starts_with("codex://") || tl.starts_with("chatgpt://") {
                continue;
            }
            if tl.contains(&flag_l) || tl == flag_l || tl.contains("remote-debugging-port") {
                return CdpArgStatus::Forwarded;
            }
        }
        return CdpArgStatus::ProtocolRedirected;
    }
    if saw_protocol {
        // Protocol present, no debug flag at all — not the owl-redirect case.
        return CdpArgStatus::NotForwarded;
    }
    CdpArgStatus::NotForwarded
}

/// Best-effort remote process command line (x64 PEB). Empty string = uninspectable.
pub fn read_process_command_line(pid: u32) -> String {
    read_process_command_line_inner(pid).unwrap_or_default()
}

#[cfg(target_arch = "x86_64")]
fn read_process_command_line_inner(pid: u32) -> Option<String> {
    // Minimal PEB walk — enough to detect owl protocol redirect on production hosts.
    #[repr(C)]
    struct ProcessBasicInformation {
        reserved1: *mut core::ffi::c_void,
        peb_base: *mut core::ffi::c_void,
        reserved2: [*mut core::ffi::c_void; 2],
        unique_process_id: usize,
        reserved3: *mut core::ffi::c_void,
    }
    #[link(name = "ntdll")]
    extern "system" {
        fn NtQueryInformationProcess(
            process: isize,
            info_class: u32,
            info: *mut core::ffi::c_void,
            info_len: u32,
            ret_len: *mut u32,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn ReadProcessMemory(
            process: isize,
            base: *const core::ffi::c_void,
            buffer: *mut core::ffi::c_void,
            size: usize,
            read: *mut usize,
        ) -> i32;
    }
    const PROCESS_BASIC_INFORMATION_CLASS: u32 = 0;
    const PROCESS_QUERY_INFORMATION: u32 = 0x0400;
    unsafe {
        let h = OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        );
        if h == 0 || h == -1 {
            return None;
        }
        let mut pbi = ProcessBasicInformation {
            reserved1: ptr::null_mut(),
            peb_base: ptr::null_mut(),
            reserved2: [ptr::null_mut(); 2],
            unique_process_id: 0,
            reserved3: ptr::null_mut(),
        };
        let mut ret_len = 0u32;
        let st = NtQueryInformationProcess(
            h,
            PROCESS_BASIC_INFORMATION_CLASS,
            &mut pbi as *mut _ as *mut _,
            std::mem::size_of::<ProcessBasicInformation>() as u32,
            &mut ret_len,
        );
        if st < 0 || pbi.peb_base.is_null() {
            CloseHandle(h);
            return None;
        }
        // PEB.ProcessParameters is at offset 0x20 on x64
        let mut params_ptr: usize = 0;
        let mut nread = 0usize;
        let ok = ReadProcessMemory(
            h,
            (pbi.peb_base as usize + 0x20) as *const _,
            &mut params_ptr as *mut _ as *mut _,
            std::mem::size_of::<usize>(),
            &mut nread,
        );
        if ok == 0 || params_ptr == 0 {
            CloseHandle(h);
            return None;
        }
        // RTL_USER_PROCESS_PARAMETERS.CommandLine UNICODE_STRING at offset 0x70
        // UNICODE_STRING: Length u16, Max u16, pad, Buffer *u16
        let mut length: u16 = 0;
        let mut buffer_ptr: usize = 0;
        let ok_len = ReadProcessMemory(
            h,
            (params_ptr + 0x70) as *const _,
            &mut length as *mut _ as *mut _,
            2,
            &mut nread,
        );
        let ok_buf = ReadProcessMemory(
            h,
            (params_ptr + 0x78) as *const _,
            &mut buffer_ptr as *mut _ as *mut _,
            std::mem::size_of::<usize>(),
            &mut nread,
        );
        if ok_len == 0 || ok_buf == 0 || buffer_ptr == 0 || length == 0 || length > 32_768 {
            CloseHandle(h);
            return None;
        }
        let wchar_count = (length as usize) / 2;
        let mut wbuf = vec![0u16; wchar_count];
        let ok_s = ReadProcessMemory(
            h,
            buffer_ptr as *const _,
            wbuf.as_mut_ptr() as *mut _,
            length as usize,
            &mut nread,
        );
        CloseHandle(h);
        if ok_s == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&wbuf))
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn read_process_command_line_inner(_pid: u32) -> Option<String> {
    None
}

/// Aggregate CDP arg status across current host processes for `port`.
pub fn inspect_host_cdp_arg_status(port: u16) -> CdpArgStatus {
    let procs = find_host_processes_toolhelp();
    if procs.is_empty() {
        return CdpArgStatus::Uninspectable;
    }
    let mut saw_uninspectable = false;
    let mut saw_not = false;
    for p in procs {
        let cmd = read_process_command_line(p.pid);
        match classify_cdp_argument_status(&cmd, port) {
            CdpArgStatus::ProtocolRedirected => return CdpArgStatus::ProtocolRedirected,
            CdpArgStatus::Forwarded => return CdpArgStatus::Forwarded,
            CdpArgStatus::NotForwarded => saw_not = true,
            CdpArgStatus::Uninspectable => saw_uninspectable = true,
        }
    }
    if saw_not {
        CdpArgStatus::NotForwarded
    } else if saw_uninspectable {
        CdpArgStatus::Uninspectable
    } else {
        CdpArgStatus::Uninspectable
    }
}

/// Prefer the registered Store package that owns a running host image path.
pub fn match_store_package_for_image_path(
    packages: &[StorePackageNative],
    image_path: &str,
) -> Option<StorePackageNative> {
    if image_path.is_empty() || packages.is_empty() {
        return None;
    }
    let img = image_path.replace('/', "\\").to_ascii_lowercase();
    for pkg in packages {
        let root = pkg.install_location.replace('/', "\\").to_ascii_lowercase();
        let exe = pkg.executable.replace('/', "\\").to_ascii_lowercase();
        if !exe.is_empty() && (img == exe || img.ends_with(&exe)) {
            return Some(pkg.clone());
        }
        if !root.is_empty() && img.starts_with(&root) {
            return Some(pkg.clone());
        }
        // full name segment in WindowsApps path
        if !pkg.package_full_name.is_empty() {
            let marker = format!("\\windowsapps\\{}", pkg.package_full_name.to_ascii_lowercase());
            if img.contains(&marker) {
                return Some(pkg.clone());
            }
        }
    }
    None
}

fn collect_descendant_pids(roots: &[u32]) -> Vec<u32> {
    if roots.is_empty() {
        return Vec::new();
    }
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == 0 || snap == -1 {
            return roots.to_vec();
        }
        let mut entry = ProcessEntry32W {
            dw_size: std::mem::size_of::<ProcessEntry32W>() as u32,
            cnt_usage: 0,
            th32_process_id: 0,
            th32_default_heap_id: 0,
            th32_module_id: 0,
            cnt_threads: 0,
            th32_parent_process_id: 0,
            pc_pri_class_base: 0,
            dw_flags: 0,
            sz_exe_file: [0; 260],
        };
        let mut rows: Vec<(u32, u32)> = Vec::new();
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                rows.push((entry.th32_process_id, entry.th32_parent_process_id));
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);

        let mut kill: HashSet<u32> = roots.iter().copied().collect();
        // Expand children a few levels (Electron helpers)
        for _ in 0..6 {
            let before = kill.len();
            for &(pid, parent) in &rows {
                if kill.contains(&parent) {
                    kill.insert(pid);
                }
            }
            if kill.len() == before {
                break;
            }
        }
        // Never kill ourselves
        let self_pid = GetCurrentProcessId();
        kill.remove(&self_pid);
        let mut all: Vec<u32> = kill.into_iter().collect();
        all.sort_unstable();
        all
    }
}

fn terminate_pid(pid: u32) -> bool {
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h == 0 || h == -1 {
            return false;
        }
        let ok = TerminateProcess(h, 1);
        // Brief wait so handles release (event-driven; max 80ms)
        let _ = WaitForSingleObject(h, 80);
        CloseHandle(h);
        ok != 0
    }
}

/// Stop host processes: Toolhelp discover → TerminateProcess tree. No taskkill, no PS.
pub fn stop_host_native() {
    let t0 = Instant::now();
    let mains = find_host_pids_toolhelp();
    if mains.is_empty() {
        append_diag("stop_host_native: no host PIDs");
        return;
    }
    append_diag(&format!(
        "stop_host_native: mains={:?} t0={}ms",
        mains,
        t0.elapsed().as_millis()
    ));
    let tree = collect_descendant_pids(&mains);
    let mut killed = 0u32;
    // Kill children first (reverse pid order heuristic), then mains
    for &pid in tree.iter().rev() {
        if terminate_pid(pid) {
            killed += 1;
        }
    }
    // Second pass on remaining mains
    for pid in find_host_pids_toolhelp() {
        if terminate_pid(pid) {
            killed += 1;
        }
    }
    // Event wait: processes gone (cap 1.2s)
    let deadline = Instant::now() + Duration::from_millis(1200);
    while Instant::now() < deadline {
        if find_host_pids_toolhelp().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    let left = find_host_pids_toolhelp();
    append_diag(&format!(
        "stop_host_native: done killed≈{killed} left={} total={}ms",
        left.len(),
        t0.elapsed().as_millis()
    ));
}

// ── Store package resolution (no PowerShell) ─────────────────────────────────

#[derive(Debug, Clone)]
pub struct StorePackageNative {
    pub aumid: String,
    pub package_full_name: String,
    pub package_family_name: String,
    pub version: String,
    pub install_location: String,
    pub executable: String,
}

const KNOWN_FAMILIES: &[&str] = &[
    // Observed OpenAI desktop family (Codex rebrand); ChatGPT variants.
    "OpenAI.Codex_2p2nqsd0c76g0",
    "OpenAI.ChatGPT_2p2nqsd0c76g0",
    "OpenAI.ChatGPT-Desktop_2p2nqsd0c76g0",
];

fn packages_for_family(family: &str) -> Vec<String> {
    let fam = wide(family);
    unsafe {
        let mut count: u32 = 0;
        let mut buf_len: u32 = 0;
        // First call: sizes
        let hr = GetPackagesByPackageFamily(
            fam.as_ptr(),
            &mut count,
            ptr::null_mut(),
            &mut buf_len,
            ptr::null_mut(),
        );
        // ERROR_INSUFFICIENT_BUFFER = 122
        if count == 0 && buf_len == 0 {
            let _ = hr;
            return Vec::new();
        }
        if count == 0 {
            return Vec::new();
        }
        let mut names: Vec<*mut u16> = vec![ptr::null_mut(); count as usize];
        let mut buffer = vec![0u16; buf_len as usize];
        let hr2 = GetPackagesByPackageFamily(
            fam.as_ptr(),
            &mut count,
            names.as_mut_ptr(),
            &mut buf_len,
            buffer.as_mut_ptr(),
        );
        if hr2 != 0 && count == 0 {
            return Vec::new();
        }
        // buffer holds concatenated full names; names[] point into buffer
        let mut out = Vec::new();
        for i in 0..(count as usize) {
            if names[i].is_null() {
                continue;
            }
            // Read null-terminated wide string at names[i]
            let mut len = 0usize;
            while *names[i].add(len) != 0 {
                len += 1;
                if len > 512 {
                    break;
                }
            }
            let slice = std::slice::from_raw_parts(names[i], len);
            out.push(String::from_utf16_lossy(slice));
        }
        out
    }
}

fn version_from_full_name(full: &str) -> String {
    // Name_Version_Arch_ResourceId_PublisherId
    full.split('_').nth(1).unwrap_or("").to_string()
}

fn install_root_for_full_name(full: &str) -> Option<PathBuf> {
    // Typical: C:\Program Files\WindowsApps\<full>
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    let p = PathBuf::from(pf).join("WindowsApps").join(full);
    if p.is_dir() {
        return Some(p);
    }
    // LOCALAPPDATA Packages (some per-user)
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let p2 = PathBuf::from(local)
            .join("Packages")
            .join(full.split('_').next().unwrap_or(full));
        if p2.is_dir() {
            return Some(p2);
        }
    }
    None
}

fn find_exe_in_install(root: &Path) -> Option<PathBuf> {
    for rel in [
        "app\\ChatGPT.exe",
        "app\\Codex.exe",
        "ChatGPT.exe",
        "Codex.exe",
    ] {
        let p = root.join(rel);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn parse_app_id_from_manifest(root: &Path) -> String {
    let manifest = root.join("AppxManifest.xml");
    let Ok(text) = std::fs::read_to_string(&manifest) else {
        return "App".into();
    };
    // Cheap extract: Id="..." on first Application
    if let Some(idx) = text.find("<Application") {
        let slice = &text[idx..];
        if let Some(id_pos) = slice.find("Id=\"") {
            let rest = &slice[id_pos + 4..];
            if let Some(end) = rest.find('"') {
                let id = &rest[..end];
                if !id.is_empty() {
                    return id.to_string();
                }
            }
        }
    }
    "App".into()
}

fn family_from_full_name(full: &str) -> String {
    // Family = Name_PublisherId (first + last underscore segments)
    let parts: Vec<&str> = full.split('_').collect();
    if parts.len() >= 2 {
        format!("{}_{}", parts[0], parts[parts.len() - 1])
    } else {
        full.to_string()
    }
}

/// All registered OpenAI Store packages (may be multiple versions during update).
pub fn list_store_packages_native() -> Vec<StorePackageNative> {
    let mut found: Vec<StorePackageNative> = Vec::new();
    let mut seen = HashSet::new();

    // 1) Known families via GetPackagesByPackageFamily
    for fam in KNOWN_FAMILIES {
        for full in packages_for_family(fam) {
            if !seen.insert(full.clone()) {
                continue;
            }
            let family = family_from_full_name(&full);
            let version = version_from_full_name(&full);
            let install = install_root_for_full_name(&full)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let exe = install_root_for_full_name(&full)
                .and_then(|r| find_exe_in_install(&r))
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let app_id = install_root_for_full_name(&full)
                .map(|r| parse_app_id_from_manifest(&r))
                .unwrap_or_else(|| "App".into());
            let aumid = format!("{family}!{app_id}");
            found.push(StorePackageNative {
                aumid,
                package_full_name: full,
                package_family_name: family,
                version,
                install_location: install,
                executable: exe,
            });
        }
    }

    // 2) Scan WindowsApps for OpenAI.* when API returned nothing or to catch extras
    if let Ok(pf) = std::env::var("ProgramFiles") {
        let wa = PathBuf::from(pf).join("WindowsApps");
        if let Ok(rd) = std::fs::read_dir(&wa) {
            for ent in rd.flatten() {
                let name = ent.file_name().to_string_lossy().to_string();
                if !name.starts_with("OpenAI.Codex_") && !name.starts_with("OpenAI.ChatGPT") {
                    continue;
                }
                if !seen.insert(name.clone()) {
                    continue;
                }
                let root = ent.path();
                if !root.is_dir() {
                    continue;
                }
                let family = family_from_full_name(&name);
                let version = version_from_full_name(&name);
                let app_id = parse_app_id_from_manifest(&root);
                let aumid = format!("{family}!{app_id}");
                let exe = find_exe_in_install(&root)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                found.push(StorePackageNative {
                    aumid,
                    package_full_name: name,
                    package_family_name: family,
                    version,
                    install_location: root.to_string_lossy().to_string(),
                    executable: exe,
                });
            }
        }
    }

    // Prefer Codex family, then highest version string
    found.sort_by(|a, b| {
        let pa = if a.package_family_name.starts_with("OpenAI.Codex") {
            0
        } else if a.package_family_name.contains("ChatGPT") {
            1
        } else {
            2
        };
        let pb = if b.package_family_name.starts_with("OpenAI.Codex") {
            0
        } else if b.package_family_name.contains("ChatGPT") {
            1
        } else {
            2
        };
        pa.cmp(&pb)
            .then_with(|| b.version.cmp(&a.version))
    });
    found
}

/// Resolve best OpenAI Store package without PowerShell.
/// When a host process is already running, prefer the package that owns that image
/// (Store auto-update may leave the old package running while "current" is newer).
pub fn resolve_store_package_native() -> Option<(StorePackageNative, u32)> {
    let found = list_store_packages_native();
    if found.is_empty() {
        return None;
    }
    let count = found.len() as u32;

    // Prefer package that owns a currently running host image.
    for proc in find_host_processes_toolhelp() {
        if let Some(pkg) = match_store_package_for_image_path(&found, &proc.image_path) {
            append_diag(&format!(
                "resolve_store_package: prefer running image pid={} full={}",
                proc.pid, pkg.package_full_name
            ));
            return Some((pkg, count.max(1)));
        }
    }

    let best = found.into_iter().next()?;
    Some((best, count.max(1)))
}

/// Activate Store app via IApplicationActivationManager (no PowerShell Add-Type).
pub fn activate_packaged_app_blocking(aumid: &str, arguments: &str) -> Result<u32, EngineError> {
    let aumid_w = wide(aumid);
    let args_w = wide(arguments);
    unsafe {
        let hr_init = CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED);
        // S_OK=0, S_FALSE=1, RPC_E_CHANGED_MODE=0x80010106 — all ok to continue
        let _ = hr_init;

        let mut punk: *mut core::ffi::c_void = ptr::null_mut();
        let hr = CoCreateInstance(
            &CLSID_APP_ACTIVATION,
            ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_IAPP_ACTIVATION,
            &mut punk,
        );
        if hr < 0 || punk.is_null() {
            CoUninitialize();
            return Err(EngineError::msg(format!(
                "CoCreateInstance ApplicationActivationManager failed hr=0x{hr:08X}"
            )));
        }
        let mgr = punk as *mut IApplicationActivationManager;
        let mut pid: u32 = 0;
        let activate = (*(*mgr).lp_vtbl).activate_application;
        let hr_act = activate(mgr, aumid_w.as_ptr(), args_w.as_ptr(), 0, &mut pid);
        // Release
        let release = (*(*mgr).lp_vtbl).release;
        release(mgr);
        CoUninitialize();

        if hr_act < 0 {
            return Err(EngineError::msg(format!(
                "ActivateApplication failed hr=0x{hr_act:08X} aumid={aumid}"
            )));
        }
        if pid == 0 {
            return Err(EngineError::msg(format!(
                "ActivateApplication returned PID 0 aumid={aumid}"
            )));
        }
        append_diag(&format!(
            "activate_packaged_app_blocking aumid={aumid} pid={pid}"
        ));
        Ok(pid)
    }
}

/// Wait for process handle signal or timeout (optional after launch).
#[allow(dead_code)]
pub fn wait_pid_brief(pid: u32, ms: u32) -> bool {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h == 0 || h == -1 {
            return false;
        }
        let r = WaitForSingleObject(h, ms);
        CloseHandle(h);
        r == WAIT_OBJECT_0 || r == WAIT_TIMEOUT
    }
}
