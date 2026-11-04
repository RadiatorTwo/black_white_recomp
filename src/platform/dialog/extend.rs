//! Our own settings inside the game's dialogs (none yet for Black & White):
//! - `adjust` changes the template before the dialog is built;
//! - `init` runs after the game's WM_INITDIALOG;
//! - `ok` runs when the dialog's OK button is pressed.

use super::template::Template;
use super::{Dialog, Notify};

pub fn adjust(_name: &str, _t: &mut Template) {}

pub(super) fn init(_d: &mut Dialog, _out: &mut Vec<Notify>) {}

pub fn ok(_d: &mut Dialog) {}
