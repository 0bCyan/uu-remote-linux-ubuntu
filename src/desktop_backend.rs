//! Select the host desktop, not the presence of an XWayland DISPLAY socket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopBackend { X11, Wayland, Portal }

/// `UU_DESKTOP_BACKEND=none`: headless host — no display is opened; terminal, file transfer and
/// port mapping stay available (servers without a monitor or logged-in session).
pub fn headless() -> bool { std::env::var("UU_DESKTOP_BACKEND").is_ok_and(|v| v == "none") }

pub fn detect() -> Result<DesktopBackend, String> {
    let backend=select(std::env::var("UU_DESKTOP_BACKEND").ok().as_deref(),
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref())?;
    let explicit=std::env::var("UU_DESKTOP_BACKEND").unwrap_or_else(|_|"auto".into());
    let desktop=std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    Ok(if explicit=="auto" && backend==DesktopBackend::Wayland && desktop.split(':').any(|s|s.eq_ignore_ascii_case("gnome")||s.eq_ignore_ascii_case("kde")) { DesktopBackend::Portal } else { backend })
}

fn select(explicit: Option<&str>, session: Option<&str>, socket: Option<&str>) -> Result<DesktopBackend, String> {
    match explicit {
        Some("x11") => Ok(DesktopBackend::X11),
        Some("wayland") => Ok(DesktopBackend::Wayland),
        Some("portal") => Ok(DesktopBackend::Portal),
        Some("none") => Ok(DesktopBackend::X11), // never opened, see headless()
        None | Some("auto") => {
            if session == Some("wayland") || socket.is_some_and(|s| !s.is_empty()) {
                Ok(DesktopBackend::Wayland)
            } else { Ok(DesktopBackend::X11) }
        },
        Some(_) => Err("UU_DESKTOP_BACKEND must be auto, x11, wayland, portal or none".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xwayland_does_not_override_wayland_session() {
        assert_eq!(select(None,Some("wayland"),Some("wayland-0")),Ok(DesktopBackend::Wayland));
        assert_eq!(select(None,None,Some("wayland-1")),Ok(DesktopBackend::Wayland));
        assert_eq!(select(None,Some("x11"),None),Ok(DesktopBackend::X11));
        assert_eq!(select(Some("x11"),Some("wayland"),Some("wayland-0")),Ok(DesktopBackend::X11));
        assert!(select(Some("typo"),None,None).is_err());
    }
}
