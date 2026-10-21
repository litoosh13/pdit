//! Undo and Redo for every change to the open document (P-050 #2): before a change, the document as it was is
//! kept ([`before_change`]). Changes are one step until the user presses again ([`new_step`]: a drag, a tool that
//! adds many things) or, without a press, while they come less than [`GROUP_MS`] apart (a burst of typing). The
//! app clears it when another file is opened ([`clear`]).

use crate::edit_open::discard_edit;
use crate::render::{open_document, with_open};
use crate::{Error, page_ops};
use std::cell::RefCell;

/// Changes closer together than this are one step.
const GROUP_MS: f64 = 1000.0;
/// At most this many steps, and about this many bytes of kept documents, are kept (the oldest go first).
const MAX_STEPS: usize = 50;
const MAX_BYTES: usize = 300 << 20;

#[derive(Default)]
struct History {
    undo: Vec<Vec<u8>>,
    redo: Vec<Vec<u8>>,
    last_change: f64,
}

thread_local! {
    static HISTORY: RefCell<History> = RefCell::new(History::default());
}

fn now() -> f64 {
    js_sys::Date::now()
}

fn current() -> Result<Vec<u8>, Error> {
    with_open(|document| Ok(document.save_to_bytes()?))
}

/// Call before changing the document: drops a previewed edit, then keeps the document as it is for Undo (unless
/// the last change was just now: then this one joins its step).
pub(crate) fn before_change() -> Result<(), Error> {
    discard_edit()?;
    let time = now();
    let joins = HISTORY.with_borrow(|h| time - h.last_change < GROUP_MS && !h.undo.is_empty());
    if !joins {
        // ponytail: a whole copy per step; fine for documents of a few MB, a 50 MB book keeps ~6 steps.
        let before = current()?;
        HISTORY.with_borrow_mut(|h| {
            h.undo.push(before);
            h.redo.clear();
            while h.undo.len() > MAX_STEPS
                || h.undo.iter().map(Vec::len).sum::<usize>() > MAX_BYTES && h.undo.len() > 1
            {
                h.undo.remove(0);
            }
        });
    }
    HISTORY.with_borrow_mut(|h| h.last_change = time);
    Ok(())
}

/// The user pressed (a click, a key): the next change is a step of its own.
pub fn new_step() {
    HISTORY.with_borrow_mut(|h| h.last_change = 0.0);
}

/// A new file: nothing to undo.
pub fn clear() {
    HISTORY.with_borrow_mut(|h| *h = History::default());
}

pub fn can_undo() -> bool {
    HISTORY.with_borrow(|h| !h.undo.is_empty())
}

pub fn can_redo() -> bool {
    HISTORY.with_borrow(|h| !h.redo.is_empty())
}

/// Goes one step back; returns the page sizes, or `None` when there is nothing to undo.
pub fn undo() -> Result<Option<Vec<(f32, f32)>>, Error> {
    step(true)
}

/// Goes one step forward again; returns the page sizes, or `None` when there is nothing to redo.
pub fn redo() -> Result<Option<Vec<(f32, f32)>>, Error> {
    step(false)
}

fn step(back: bool) -> Result<Option<Vec<(f32, f32)>>, Error> {
    discard_edit()?;
    let now_bytes = current()?;
    loop {
        let Some(bytes) =
            HISTORY.with_borrow_mut(|h| if back { h.undo.pop() } else { h.redo.pop() })
        else {
            return Ok(None);
        };
        // A step that changed nothing (a preview put back) is skipped.
        if bytes == now_bytes {
            continue;
        }
        open_document(bytes)?;
        HISTORY.with_borrow_mut(|h| {
            if back {
                h.redo.push(now_bytes);
            } else {
                h.undo.push(now_bytes);
            }
            // The next change is a step of its own.
            h.last_change = 0.0;
        });
        return page_ops::page_sizes().map(Some);
    }
}

/// Replaces the open document with `bytes` (an edit made elsewhere, e.g. by MuPDF on the desktop), as one step.
pub fn replace_document(bytes: Vec<u8>) -> Result<Vec<(f32, f32)>, Error> {
    before_change()?;
    open_document(bytes)?;
    page_ops::page_sizes()
}
