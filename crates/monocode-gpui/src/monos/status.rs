//! AppKit owns menu navigation and accessibility; actions return to GPUI.
use monocode_core::menu_bar::{self as live, Item as LiveItem};
use objc2::runtime::AnyObject;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained, sel,
};
use objc2_app_kit::{
    NSControlStateValueOn, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem,
};
use objc2_foundation::{NSData, NSSize, NSString};
use std::rc::Rc;
struct Action {
    key: String,
    run: Rc<dyn Fn(&str)>,
}
define_class!(
    #[unsafe(super(NSMenuItem))]
    #[name="MonocodeMonoMenuItem"]
    #[thread_kind=MainThreadOnly]
    #[ivars=Action]
    struct Item;
    impl Item{
        #[unsafe(method(runMonoAction:))]
        fn run(&self,_:Option<&AnyObject>){(self.ivars().run)(&self.ivars().key);}
    }
);
impl Item {
    fn new(
        mtm: MainThreadMarker,
        title: &str,
        key: String,
        run: Rc<dyn Fn(&str)>,
    ) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(Action { key, run });
        let item: Retained<Self> = unsafe {
            msg_send![super(this),initWithTitle:&*NSString::from_str(title),action:Some(sel!(runMonoAction:)),keyEquivalent:&*NSString::from_str("")]
        };
        // The status menu retains each item for as long as it can be invoked.
        unsafe {
            item.setTarget(Some(&item));
        }
        item
    }
}
pub(super) struct Status {
    item: Retained<NSStatusItem>,
    signature: String,
    /// monoBOT's mark alone, and what is drawn beside it.
    icon: Option<Retained<NSImage>>,
    live: Option<LiveItem>,
}
impl Status {
    pub(super) fn new() -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.);
        let mut icon = None;
        if let Some(button) = item.button(mtm) {
            let data =
                NSData::with_bytes(include_bytes!("../../../../assets/macos/status-icon.png"));
            if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
                image.setSize(NSSize::new(18., 18.));
                image.setTemplate(true);
                button.setImage(Some(&image));
                icon = Some(image);
            } else {
                button.setTitle(&NSString::from_str("Mono"));
            }
            button.setToolTip(Some(&NSString::from_str("monoBOT")));
        }
        Some(Self {
            item,
            signature: String::new(),
            icon,
            live: None,
        })
    }
    /// Draws the live item beside the mark, as part of the icon, or the mark
    /// alone.
    pub(super) fn set_live(&mut self, item: Option<&LiveItem>) {
        if self.live.as_ref() == item {
            return;
        }
        self.live = item.cloned();
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let (Some(button), Some(icon)) = (self.item.button(mtm), &self.icon) else {
            return;
        };
        match item {
            Some(item) => {
                button.setImage(Some(&crate::menu_bar::mac::compose(icon, item)));
                button.setToolTip(Some(&NSString::from_str(&item.tooltip)));
            }
            None => {
                button.setImage(Some(icon));
                button.setToolTip(Some(&NSString::from_str("monoBOT")));
            }
        }
    }
    pub(super) fn update(
        &mut self,
        monos: &[(String, String)],
        beside: &str,
        run: Rc<dyn Fn(&str)>,
    ) {
        let signature = format!("{monos:?} {}", live::normalized(beside));
        if self.signature == signature {
            return;
        }
        self.signature = signature;
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let entries = crate::menu_bar::status_menu(monos, beside, false);
        let menu = build(mtm, &entries, &run);
        self.item.setMenu(Some(&menu));
    }
}
impl Drop for Status {
    fn drop(&mut self) {
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}

/// An AppKit menu for the icon menu's entries (`crate::menu_bar::Entry`).
fn build(
    mtm: MainThreadMarker,
    entries: &[crate::menu_bar::Entry],
    run: &Rc<dyn Fn(&str)>,
) -> Retained<NSMenu> {
    use crate::menu_bar::Entry;
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    for entry in entries {
        match entry {
            Entry::Action { label, id, checked } => {
                let item = Item::new(mtm, label, id.clone(), run.clone());
                if *checked {
                    item.setState(NSControlStateValueOn);
                }
                menu.addItem(&item);
            }
            Entry::Note(label) => {
                let note = NSMenuItem::new(mtm);
                note.setTitle(&NSString::from_str(label));
                note.setEnabled(false);
                menu.addItem(&note);
            }
            Entry::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
            Entry::Submenu { label, entries } => {
                let parent = NSMenuItem::new(mtm);
                parent.setTitle(&NSString::from_str(label));
                parent.setSubmenu(Some(&build(mtm, entries, run)));
                menu.addItem(&parent);
            }
        }
    }
    menu
}
