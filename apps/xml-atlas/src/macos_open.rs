//! Files from Finder, the Dock and `open` on macOS (`docs/decisions.md`, D26).
//!
//! macOS hands such files to the app as an "open documents" Apple event, which winit does not
//! pass on. This module installs a handler for that event, when the app is about to finish
//! launching (after AppKit's own handlers, before the event for the files given at launch),
//! and sends each file's path over a channel to the app, which opens it as a tab.
//!
//! The only `unsafe` code of the workspace is here (D26); every block says why it is sound.
#![allow(unsafe_code)]

use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, Sender, channel};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::NSApplicationWillFinishLaunchingNotification;
use objc2_foundation::{
    NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter, NSObject,
    NSObjectProtocol,
};

/// `kCoreEventClass`, `'aevt'`: the class of the core Apple events.
const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");

/// `kAEOpenDocuments`, `'odoc'`: open documents.
const OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");

/// `keyDirectObject`, `'----'`: the event's direct object, here the files.
const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

/// The window to wake when a file arrives, once there is one.
static CONTEXT: OnceLock<egui::Context> = OnceLock::new();

/// What the handler keeps: where to send the paths.
struct Ivars {
    sender: Sender<PathBuf>,
}

define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - `OpenHandler` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "XmlAtlasOpenHandler"]
    #[ivars = Ivars]
    struct OpenHandler;

    impl OpenHandler {
        // SAFETY: the signature is the one NSAppleEventManager calls a handler with: the
        // event and the reply, both descriptors, and no return value.
        #[unsafe(method(handleOpen:withReply:))]
        fn handle_open(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            let paths = files(event);
            log::info!("asked by macOS to open {paths:?}");
            for path in paths {
                // The app may be quitting; then there is nothing to open the file in.
                let _ = self.ivars().sender.send(path);
            }
            if let Some(ctx) = CONTEXT.get() {
                ctx.request_repaint();
            }
        }
    }

    unsafe impl NSObjectProtocol for OpenHandler {}
);

/// Starts listening for files from Finder; call before eframe starts. The paths come out of
/// the receiver; files given at launch are there before the first frame.
pub fn listen() -> Receiver<PathBuf> {
    let (sender, receiver) = channel();
    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
        // The notification comes on the main thread, where AppKit posts it.
        let Some(mtm) = MainThreadMarker::new() else {
            log::warn!("not on the main thread: files from Finder will not open");
            return;
        };
        let handler = OpenHandler::new(mtm, sender.clone());
        let manager = NSAppleEventManager::sharedAppleEventManager();
        // SAFETY: `handler` responds to `handleOpen:withReply:` with the signature the
        // manager calls (see `handle_open`). The event class and id are `u32`, as
        // `AEEventClass` and `AEEventID` are.
        let () = unsafe {
            msg_send![
                &manager,
                setEventHandler: &*handler,
                andSelector: sel!(handleOpen:withReply:),
                forEventClass: CORE_EVENT_CLASS,
                andEventID: OPEN_DOCUMENTS
            ]
        };
        // The manager does not keep its handler: it lives as long as the app.
        std::mem::forget(handler);
    });
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: reading an AppKit constant, a string that is never written.
    let name = unsafe { NSApplicationWillFinishLaunchingNotification };
    // SAFETY: no object to match and no queue, so the block runs on the thread that posts
    // the notification, the main thread. The block holds only a `Sender`, which is `Send`.
    let observer = unsafe {
        center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
    };
    // The observer must stay registered until the app has launched; it costs nothing after.
    std::mem::forget(observer);
    receiver
}

/// The window to wake when files arrive while the app runs.
pub fn wake(ctx: &egui::Context) {
    let _ = CONTEXT.set(ctx.clone());
}

impl OpenHandler {
    fn new(mtm: MainThreadMarker, sender: Sender<PathBuf>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars { sender });
        // SAFETY: `init` of NSObject, the superclass, on a freshly allocated object.
        unsafe { msg_send![super(this), init] }
    }
}

/// The paths of the files in an "open documents" event: a list of file URLs, or one.
fn files(event: &NSAppleEventDescriptor) -> Vec<PathBuf> {
    // SAFETY: `paramDescriptorForKeyword:` takes an `AEKeyword`, a `u32`, and returns a
    // descriptor or nil.
    let list: Option<Retained<NSAppleEventDescriptor>> =
        unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] };
    let Some(list) = list else {
        return Vec::new();
    };
    let count = list.numberOfItems();
    let items: Vec<_> = if count > 0 {
        // Descriptor lists count from 1.
        (1..=count)
            .filter_map(|i| list.descriptorAtIndex(i))
            .collect()
    } else {
        vec![list]
    };
    items
        .iter()
        .filter_map(|item| item.fileURLValue())
        .filter_map(|url| url.to_file_path())
        .collect()
}
