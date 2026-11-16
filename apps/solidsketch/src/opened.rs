//! The documents the system asks the app to open (desktop): a `.ssk` file double-clicked in the
//! Finder, the Explorer or a file manager, dropped on the app's icon, or named on the command
//! line. They wait here until the app's next frame opens them, in tabs of their own.
//!
//! - Windows, Linux: the file associations (packaging/) start the app with the files' paths as
//!   arguments, a new instance each time.
//! - macOS: Launch Services sends the running app (just launched, or already open) an Apple
//!   Event « open documents ». winit's application delegate does not handle it, so the app
//!   installs its own handler, when the app will finish launching: AppKit installs its default
//!   ones just before, and the launch's event comes just after.

use std::sync::{Mutex, OnceLock};

static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());
static CONTEXT: OnceLock<egui::Context> = OnceLock::new();

/// Queues `paths`, waking the app if it runs.
fn push(paths: impl IntoIterator<Item = String>) {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    let before = pending.len();
    pending.extend(paths);
    if pending.len() > before
        && let Some(ctx) = CONTEXT.get()
    {
        ctx.request_repaint();
    }
}

/// Before the event loop: queues the files named on the command line, and on macOS installs
/// the handler of the « open documents » event.
pub fn install() {
    push(std::env::args().skip(1).filter(|a| !a.starts_with('-') && std::path::Path::new(a).is_file()));
    #[cfg(target_os = "macos")]
    macos::install();
}

/// The app's context, to wake it when files arrive.
pub fn set_context(ctx: &egui::Context) {
    let _ = CONTEXT.set(ctx.clone());
}

/// The files waiting to be opened, in the order they came.
pub fn take() -> Vec<String> {
    std::mem::take(&mut *PENDING.lock().unwrap_or_else(|e| e.into_inner()))
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{AnyThread, define_class, msg_send, sel};
    use objc2_app_kit::NSApplicationWillFinishLaunchingNotification;
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSURL};

    /// `kCoreEventClass`, `kAEOpenDocuments`, `keyDirectObject` (Apple Events' four-char codes).
    const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
    const OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
    const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

    define_class!(
        /// Observes the app's launch, then handles the « open documents » events.
        #[unsafe(super(NSObject))]
        #[name = "SolidSketchOpenDocuments"]
        struct Opener;

        impl Opener {
            #[unsafe(method(willFinishLaunching:))]
            fn will_finish_launching(&self, _notification: &NSNotification) {
                unsafe {
                    let manager: *mut AnyObject = msg_send![objc2::class!(NSAppleEventManager), sharedAppleEventManager];
                    let _: () = msg_send![
                        manager,
                        setEventHandler: self,
                        andSelector: sel!(handleOpenDocuments:withReplyEvent:),
                        forEventClass: CORE_EVENT_CLASS,
                        andEventID: OPEN_DOCUMENTS
                    ];
                }
            }

            #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
            fn handle_open_documents(&self, event: &AnyObject, _reply: &AnyObject) {
                super::push(paths(event));
            }
        }
    );

    /// The paths of the files of an « open documents » event: its direct object, a list of file
    /// URLs (or a single one).
    fn paths(event: &AnyObject) -> Vec<String> {
        let mut paths = Vec::new();
        unsafe {
            let list: *mut AnyObject = msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT];
            let Some(list) = list.as_ref() else { return paths };
            let count: isize = msg_send![list, numberOfItems];
            // A single file may come as the descriptor itself (0 items).
            let items: Vec<*mut AnyObject> = if count == 0 {
                vec![list as *const AnyObject as *mut AnyObject]
            } else {
                (1..=count).map(|i| msg_send![list, descriptorAtIndex: i]).collect()
            };
            for item in items {
                let Some(item) = item.as_ref() else { continue };
                let url: Option<Retained<NSURL>> = msg_send![item, fileURLValue];
                if let Some(path) = url.and_then(|u| u.path()) {
                    paths.push(path.to_string());
                }
            }
        }
        paths
    }

    pub fn install() {
        let opener: Retained<Opener> = unsafe { msg_send![Opener::alloc(), init] };
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                &opener,
                sel!(willFinishLaunching:),
                Some(NSApplicationWillFinishLaunchingNotification),
                None,
            );
        }
        // The center does not retain its observers: this one lives as long as the app.
        std::mem::forget(opener);
    }
}
