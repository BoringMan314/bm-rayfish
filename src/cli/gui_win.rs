//! Windows desktop chrome for `ray gui`: single-instance handshake, tray, and
//! a fixed-size WebView2 window.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

use super::gui_settings::{GuiShared, GuiWake, PendingUpdate};
use super::open_url;

const MUTEX_NAME: &str = r"Global\bm-rayfish";
const PIPE_NAME: &str = r"\\.\pipe\bm-rayfish";
const GITHUB_URL: &str = "https://github.com/BoringMan314/bm-rayfish";
const ABOUT_URL: &str = "http://exnormal.com:81/";
const UPDATE_REPO: &str = "BoringMan314/bm-rayfish";
const WIN_WIDTH: f64 = 860.0;
const WIN_HEIGHT_MIN: f64 = 280.0;
const WIN_HEIGHT_MAX: f64 = 800.0;
const WIN_HEIGHT_COMPACT: f64 = 320.0;

pub(crate) struct MutexHold(isize);

impl Drop for MutexHold {
    fn drop(&mut self) {
        unsafe {
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::Threading::ReleaseMutex;
            if self.0 != 0 {
                let h = self.0 as windows_sys::Win32::Foundation::HANDLE;
                let _ = ReleaseMutex(h);
                let _ = CloseHandle(h);
            }
        }
    }
}

#[derive(Clone)]
enum UserEvent {
    Quit,
    #[allow(dead_code)]
    Restore,
    #[allow(dead_code)]
    OpenGithub,
    #[allow(dead_code)]
    OpenAbout,
    #[allow(dead_code)]
    DownloadUpdate,
    LocaleChanged,
    UpdateChanged,
    SetHeight(f64),
}

struct TrayBits {
    tray: tray_icon::TrayIcon,
    download: Option<tray_icon::menu::MenuItem>,
    github: tray_icon::menu::MenuItem,
    about: tray_icon::menu::MenuItem,
    exit: tray_icon::menu::MenuItem,
}

pub(crate) fn take_mutex() -> Option<MutexHold> {
    take_mutex_inner().map(MutexHold)
}

fn take_mutex_inner() -> Option<isize> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
    use windows_sys::Win32::System::Threading::{CreateMutexW, OpenMutexW, WaitForSingleObject};

    const ERROR_ALREADY_EXISTS: u32 = 183;
    let name = encode_wide(MUTEX_NAME);
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        unsafe {
            let h = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
            if h.is_null() {
                return None;
            }
            if GetLastError() != ERROR_ALREADY_EXISTS {
                return Some(h as isize);
            }
            let _ = CloseHandle(h);
            if Instant::now() >= deadline {
                let h = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
                if h.is_null() {
                    return None;
                }
                return Some(h as isize);
            }
            let existing = OpenMutexW(SYNCHRONIZE, 0, name.as_ptr());
            if !existing.is_null() {
                let _ = WaitForSingleObject(existing, 200);
                let _ = CloseHandle(existing);
            }
        }
        notify_peer_quit();
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub(crate) fn run_native_window(url: &str, shared: Arc<Mutex<GuiShared>>) -> Result<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    {
        let wake_proxy = proxy.clone();
        if let Ok(mut g) = shared.lock() {
            g.wake = Some(Arc::new(move |w| {
                let ev = match w {
                    GuiWake::LocaleChanged => UserEvent::LocaleChanged,
                    GuiWake::UpdateChanged => UserEvent::UpdateChanged,
                };
                let _ = wake_proxy.send_event(ev);
            }));
        }
    }
    start_pipe_server(proxy.clone());
    spawn_update_check(shared.clone());

    let (title, tooltip) = {
        let g = shared.lock().unwrap_or_else(|e| e.into_inner());
        let t = g.config.window_title();
        (t.clone(), t)
    };

    let window_icon = load_window_icon().ok();
    let mut builder = WindowBuilder::new()
        .with_title(&title)
        .with_inner_size(LogicalSize::new(WIN_WIDTH, WIN_HEIGHT_COMPACT))
        .with_resizable(false)
        .with_maximizable(false)
        .with_position(PhysicalPosition::new(100, 100));
    if let Some(icon) = window_icon {
        builder = builder.with_window_icon(Some(icon));
    }
    let window = builder.build(&event_loop).context("creating GUI window")?;
    let ipc_proxy = proxy.clone();
    let _webview = WebViewBuilder::new()
        .with_url(url)
        .with_ipc_handler(move |req| {
            let body = req.body();
            if let Some(rest) = body.strip_prefix("height:")
                && let Ok(h) = rest.parse::<f64>()
            {
                let _ = ipc_proxy.send_event(UserEvent::SetHeight(h));
            }
        })
        .build(&window)
        .context("creating WebView2 (install the Evergreen WebView2 runtime if this fails)")?;

    let mut tray = match load_tray_icon() {
        Ok(icon) => make_tray(&shared, icon, &tooltip).ok(),
        Err(_) => None,
    };

    let menu_rx = tray_icon::menu::MenuEvent::receiver();
    let tray_rx = tray_icon::TrayIconEvent::receiver();
    let mut show_update_title = false;
    let mut next_tick = Instant::now() + Duration::from_secs(3);

    event_loop.run(move |event, _, control_flow| {
        let has_update = shared
            .lock()
            .ok()
            .and_then(|g| g.update.as_ref().map(|_| ()))
            .is_some();
        if has_update {
            *control_flow = ControlFlow::WaitUntil(next_tick);
        } else {
            *control_flow = ControlFlow::Wait;
            show_update_title = false;
        }

        while let Ok(ev) = tray_rx.try_recv() {
            if let tray_icon::TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                button_state: tray_icon::MouseButtonState::Up,
                ..
            } = ev
            {
                restore_window(&window);
            }
        }
        while let Ok(ev) = menu_rx.try_recv() {
            if let Some(bits) = tray.as_ref() {
                if ev.id == bits.github.id() {
                    let _ = open_url(GITHUB_URL);
                } else if ev.id == bits.about.id() {
                    let _ = open_url(ABOUT_URL);
                } else if ev.id == bits.exit.id() {
                    *control_flow = ControlFlow::Exit;
                } else if bits.download.as_ref().is_some_and(|d| ev.id == d.id()) {
                    start_download(&shared);
                }
            }
        }

        match event {
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) if has_update => {
                show_update_title = !show_update_title;
                next_tick = Instant::now() + Duration::from_secs(3);
                apply_titles(&window, tray.as_mut(), &shared, show_update_title);
            }
            Event::UserEvent(UserEvent::Quit) => *control_flow = ControlFlow::Exit,
            Event::UserEvent(UserEvent::Restore) => restore_window(&window),
            Event::UserEvent(UserEvent::OpenGithub) => {
                let _ = open_url(GITHUB_URL);
            }
            Event::UserEvent(UserEvent::OpenAbout) => {
                let _ = open_url(ABOUT_URL);
            }
            Event::UserEvent(UserEvent::DownloadUpdate) => start_download(&shared),
            Event::UserEvent(UserEvent::SetHeight(h)) => {
                apply_inner_size(&window, h);
            }
            Event::UserEvent(UserEvent::LocaleChanged)
            | Event::UserEvent(UserEvent::UpdateChanged) => {
                if let Some(bits) = tray.as_mut() {
                    rebuild_tray(bits, &shared);
                }
                apply_titles(&window, tray.as_mut(), &shared, show_update_title);
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            Event::WindowEvent {
                event: WindowEvent::Moved(_) | WindowEvent::Resized(_),
                ..
            } if window.is_minimized() => {
                window.set_visible(false);
            }
            _ => {}
        }
    });
}

fn apply_titles(
    window: &tao::window::Window,
    tray: Option<&mut TrayBits>,
    shared: &Arc<Mutex<GuiShared>>,
    show_update_title: bool,
) {
    let Ok(g) = shared.lock() else {
        return;
    };
    let product = g.config.window_title();
    if show_update_title {
        if let Some(upd) = &g.update {
            window.set_title(
                &g.config
                    .t("update_available")
                    .replace("{ver}", &upd.version),
            );
        } else {
            window.set_title(&product);
        }
    } else {
        window.set_title(&product);
    }
    if let Some(bits) = tray {
        let _ = bits.tray.set_tooltip(Some(&product));
    }
}

fn apply_inner_size(window: &tao::window::Window, height: f64) {
    let h = height.clamp(WIN_HEIGHT_MIN, WIN_HEIGHT_MAX);
    // Non-resizable windows on Win32 often ignore set_inner_size unless
    // WS_THICKFRAME is briefly restored.
    window.set_resizable(true);
    window.set_inner_size(LogicalSize::new(WIN_WIDTH, h));
    window.set_resizable(false);
}

fn restore_window(window: &tao::window::Window) {
    window.set_visible(true);
    window.set_minimized(false);
    window.set_outer_position(PhysicalPosition::new(100, 100));
    window.set_focus();
    #[cfg(windows)]
    {
        use tao::platform::windows::WindowExtWindows;
        use windows_sys::Win32::UI::WindowsAndMessaging::{BringWindowToTop, SetForegroundWindow};
        let hwnd = window.hwnd() as windows_sys::Win32::Foundation::HWND;
        unsafe {
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

fn make_tray(
    shared: &Arc<Mutex<GuiShared>>,
    icon: tray_icon::Icon,
    tooltip: &str,
) -> Result<TrayBits> {
    let (menu, bits_partial) = build_menu_items(shared);
    let tray = tray_icon::TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip)
        .with_icon(icon)
        .build()
        .context("creating tray icon")?;
    Ok(TrayBits {
        tray,
        download: bits_partial.0,
        github: bits_partial.1,
        about: bits_partial.2,
        exit: bits_partial.3,
    })
}

fn build_menu_items(
    shared: &Arc<Mutex<GuiShared>>,
) -> (
    tray_icon::menu::Menu,
    (
        Option<tray_icon::menu::MenuItem>,
        tray_icon::menu::MenuItem,
        tray_icon::menu::MenuItem,
        tray_icon::menu::MenuItem,
    ),
) {
    use tray_icon::menu::{Menu, MenuItem};
    let g = shared.lock().unwrap_or_else(|e| e.into_inner());
    let download = if g.update.is_some() {
        Some(MenuItem::new(g.config.t("download_update"), true, None))
    } else {
        None
    };
    let github = MenuItem::new("GitHub", true, None);
    let about = MenuItem::new(g.config.t("about"), true, None);
    let exit = MenuItem::new(g.config.t("exit"), true, None);
    drop(g);
    let menu = Menu::new();
    if let Some(d) = download.as_ref() {
        let _ = menu.append(d);
    }
    let _ = menu.append(&github);
    let _ = menu.append(&about);
    let _ = menu.append(&exit);
    (menu, (download, github, about, exit))
}

fn rebuild_tray(bits: &mut TrayBits, shared: &Arc<Mutex<GuiShared>>) {
    let (menu, parts) = build_menu_items(shared);
    bits.download = parts.0;
    bits.github = parts.1;
    bits.about = parts.2;
    bits.exit = parts.3;
    bits.tray.set_menu(Some(Box::new(menu)));
    if let Ok(g) = shared.lock() {
        let _ = bits.tray.set_tooltip(Some(g.config.window_title()));
    }
}

fn start_download(shared: &Arc<Mutex<GuiShared>>) {
    let Some(upd) = shared.lock().ok().and_then(|g| {
        g.update
            .as_ref()
            .map(|u| (u.url.clone(), u.file_name.clone()))
    }) else {
        return;
    };
    std::thread::spawn(move || {
        let _ = download_zip(&upd.0, &upd.1);
    });
}

fn download_zip(url: &str, file_name: &str) -> Result<()> {
    let dir = std::env::current_exe()?
        .parent()
        .map(|p| p.to_path_buf())
        .context("exe dir")?;
    let dest = dir.join(file_name);
    if dest.exists() {
        return Ok(());
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .user_agent("bm-rayfish")
            .timeout(Duration::from_secs(60))
            .build()?;
        let bytes = client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        std::fs::write(&dest, bytes)?;
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

fn spawn_update_check(shared: Arc<Mutex<GuiShared>>) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        if let Some(upd) = check_latest()
            && let Ok(mut g) = shared.lock()
        {
            g.update = Some(upd);
            if let Some(wake) = &g.wake {
                wake(GuiWake::UpdateChanged);
            }
        }
    });
}

#[derive(serde::Deserialize)]
struct LatestRelease {
    tag_name: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(serde::Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

fn check_latest() -> Option<PendingUpdate> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    rt.block_on(async {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .user_agent("bm-rayfish")
            .timeout(Duration::from_secs(15))
            .build()
            .ok()?;
        let api = format!("https://api.github.com/repos/{UPDATE_REPO}/releases/latest");
        let rel: LatestRelease = client
            .get(&api)
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()?;
        let latest = rayfish::update::normalize_version(&rel.tag_name);
        if !rayfish::update::version_is_newer(latest, env!("CARGO_PKG_VERSION")) {
            return None;
        }
        let asset = rel.assets.iter().find(|a| {
            let n = a.name.to_ascii_lowercase();
            n.ends_with(".zip")
        })?;
        let version = latest.to_string();
        Some(PendingUpdate {
            version: version.clone(),
            url: asset.browser_download_url.clone(),
            file_name: format!("bm-rayfish-V{version}.zip"),
        })
    })
}

fn load_tray_icon() -> Result<tray_icon::Icon> {
    let img = image::load_from_memory(include_bytes!("../../icons/icon.png"))?.into_rgba8();
    let (w, h) = img.dimensions();
    tray_icon::Icon::from_rgba(img.into_raw(), w, h).context("tray icon rgba")
}

fn load_window_icon() -> Result<tao::window::Icon> {
    let img = image::load_from_memory(include_bytes!("../../icons/icon.png"))?.into_rgba8();
    let (w, h) = img.dimensions();
    tao::window::Icon::from_rgba(img.into_raw(), w, h).context("window icon rgba")
}

fn encode_wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn notify_peer_quit() {
    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        WriteFile,
    };
    use windows_sys::Win32::System::Pipes::WaitNamedPipeW;

    let name = encode_wide(PIPE_NAME);
    for _ in 0..40 {
        unsafe {
            let h = CreateFileW(
                name.as_ptr(),
                GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            );
            if h != INVALID_HANDLE_VALUE {
                let byte = [0x7Eu8];
                let mut written = 0u32;
                let _ = WriteFile(h, byte.as_ptr(), 1, &mut written, std::ptr::null_mut());
                let _ = CloseHandle(h);
                return;
            }
            let _ = WaitNamedPipeW(name.as_ptr(), 250);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn start_pipe_server(proxy: tao::event_loop::EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::Storage::FileSystem::{PIPE_ACCESS_INBOUND, ReadFile};
        use windows_sys::Win32::System::Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
            PIPE_TYPE_BYTE, PIPE_WAIT,
        };

        let name = encode_wide(PIPE_NAME);
        loop {
            unsafe {
                let pipe = CreateNamedPipeW(
                    name.as_ptr(),
                    PIPE_ACCESS_INBOUND,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    1,
                    16,
                    16,
                    0,
                    std::ptr::null(),
                );
                if pipe.is_null() || pipe == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                }
                let _ = ConnectNamedPipe(pipe, std::ptr::null_mut());
                let mut buf = [0u8; 1];
                let mut read = 0u32;
                let ok = ReadFile(pipe, buf.as_mut_ptr(), 1, &mut read, std::ptr::null_mut());
                let _ = DisconnectNamedPipe(pipe);
                let _ = CloseHandle(pipe);
                if ok != 0 && read == 1 && buf[0] == 0x7E {
                    let _ = proxy.send_event(UserEvent::Quit);
                    break;
                }
            }
        }
    });
}
