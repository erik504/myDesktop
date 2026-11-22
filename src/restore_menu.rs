//! Submenus for restoring one entry from the default layout.
use crate::{core, Handle, MF_STRING};

const MF_GRAYED: u32 = 0x1;
const MF_POPUP: u32 = 0x10;
const FIRST_WINDOW_COMMAND: u32 = 100;

struct Popup(Handle);

impl Popup {
    fn new() -> Option<Self> {
        let handle = unsafe { crate::CreatePopupMenu() };
        if handle.is_null() {
            None
        } else {
            Some(Self(handle))
        }
    }

    fn append(&self, flags: u32, id: usize, label: &str) -> bool {
        let text = crate::wide(label);
        unsafe { crate::AppendMenuW(self.0, flags, id, text.as_ptr()) != 0 }
    }

    fn attach(self, parent: Handle, label: &str) -> bool {
        let text = crate::wide(label);
        if unsafe { crate::AppendMenuW(parent, MF_POPUP, self.0 as usize, text.as_ptr()) } == 0 {
            return false;
        }
        std::mem::forget(self); // DestroyMenu on the parent releases attached submenus.
        true
    }
}

impl Drop for Popup {
    fn drop(&mut self) {
        unsafe {
            crate::DestroyMenu(self.0);
        }
    }
}

fn menu_label(label: &str) -> String {
    const MAX_CHARS: usize = 100;
    let mut text: String = label
        .chars()
        .take(MAX_CHARS)
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    if label.chars().nth(MAX_CHARS).is_some() {
        text.push('…');
    }
    if text.trim().is_empty() {
        return "(Senza titolo)".into();
    }
    text.replace('&', "&&")
}

pub fn append_to(parent: Handle) -> Option<core::Layout> {
    const LABEL: &str = "Ripristina finestra da lista...";
    let layout = core::default_layout();
    let built = (|| {
        let list = Popup::new()?;
        match &layout {
            Ok(layout) => {
                for group in layout.groups() {
                    let program = Popup::new()?;
                    for (index, title) in group.windows {
                        let id = FIRST_WINDOW_COMMAND.checked_add(u32::try_from(index).ok()?)?;
                        if id > u32::from(u16::MAX)
                            || !program.append(MF_STRING, id as usize, &menu_label(&title))
                        {
                            return None;
                        }
                    }
                    if !program.attach(list.0, &menu_label(&group.program)) {
                        return None;
                    }
                }
            }
            Err(message) => {
                if !list.append(MF_GRAYED, 0, &menu_label(message)) {
                    return None;
                }
            }
        }
        list.attach(parent, LABEL).then_some(())
    })();
    if built.is_none() {
        unsafe {
            crate::AppendMenuW(parent, MF_GRAYED, 0, crate::wide(LABEL).as_ptr());
        }
        return None;
    }
    layout.ok()
}

pub fn window_index(command: u32) -> Option<usize> {
    command
        .checked_sub(FIRST_WINDOW_COMMAND)
        .map(|index| index as usize)
}
