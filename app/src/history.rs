//! `HistoryController` for QML: Rust-backed undo/redo timeline for photo recipes.
//!
//! Storing recipe snapshots in Rust avoids repeated JS `JSON.parse(JSON.stringify(...))`
//! deep-cloning churn on every slider drag while keeping the timeline bounded to
//! `MAX_HISTORY` entries and ignoring culling metadata (`rating`, `flag`) in edit diffs.

use std::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;
use serde::Serialize;
use serde_json::Value;

const MAX_HISTORY: usize = 100;

const IGNORED_DIFF_KEYS: &[&str] = &["rating", "flag", "path", "label"];
const SINGLE_NUMERIC_KEYS: &[&str] = &[
    "exposure",
    "contrast",
    "highlights",
    "shadows",
    "whites",
    "blacks",
    "texture",
    "clarity",
    "dehaze",
    "vibrance",
    "saturation",
];

#[cxx_qt::bridge]
pub mod qobject {
    // SAFETY: Qt C++ headers and types match cxx-qt-lib bindings for the lifetime of the Qt app.
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, undo_count, READ, NOTIFY)]
        #[qproperty(i32, redo_count, READ, NOTIFY)]
        #[qproperty(bool, can_undo, READ, NOTIFY)]
        #[qproperty(bool, can_redo, READ, NOTIFY)]
        #[qproperty(i32, revision, READ, NOTIFY)]
        type HistoryController = super::HistoryControllerRust;

        /// Clears all undo/redo entries and baseline (called when switching photos).
        #[qinvokable]
        fn reset(self: Pin<&mut HistoryController>);

        /// Records the current programmatic recipe as the new undo baseline without pushing.
        #[qinvokable]
        fn set_baseline(self: Pin<&mut HistoryController>, recipe_json: &QString);

        /// Commits any pending edit diff, then pushes `current_recipe_json` onto the undo stack.
        #[qinvokable]
        fn push_state(self: Pin<&mut HistoryController>, current_recipe_json: &QString);

        /// Commits the pre-edit baseline as a single undo step if `current_recipe_json` differs.
        #[qinvokable]
        fn commit_pending(self: Pin<&mut HistoryController>, current_recipe_json: &QString) -> bool;

        /// Steps backward one state and returns its JSON, or `""` when the undo stack is empty.
        #[qinvokable]
        fn undo(self: Pin<&mut HistoryController>, current_recipe_json: &QString) -> QString;

        /// Steps forward one state and returns its JSON, or `""` when the redo stack is empty.
        #[qinvokable]
        fn redo(self: Pin<&mut HistoryController>, current_recipe_json: &QString) -> QString;

        /// Jumps directly to timeline `index` (`0..=undo_count+redo_count`) and returns its JSON,
        /// or `""` when out of range or already at `index`.
        #[qinvokable]
        fn jump_to(self: Pin<&mut HistoryController>, index: i32, current_recipe_json: &QString) -> QString;

        /// Computes structured diff descriptors for `[undo..., current, redo.rev()...]` as JSON
        /// so QML only localizes keys without walking or stringifying full recipe trees.
        #[qinvokable]
        fn diff_descriptors(self: &HistoryController, current_recipe_json: &QString) -> QString;
    }
}

#[derive(Default)]
pub struct HistoryControllerRust {
    undo_count: i32,
    redo_count: i32,
    can_undo: bool,
    can_redo: bool,
    revision: i32,
    state: HistoryState,
}

#[derive(Debug, Default)]
pub(crate) struct HistoryState {
    undo_stack: Vec<Value>,
    redo_stack: Vec<Value>,
    baseline: Option<Value>,
}

#[derive(Debug, Serialize, PartialEq)]
struct DiffDescriptor {
    kind: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    keys: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    single_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    single_value: Option<f64>,
}

fn parse_recipe(json: &str) -> Option<Value> {
    let val: Value = serde_json::from_str(json).ok()?;
    val.is_object().then_some(val)
}

/// Compares two recipe JSON objects while ignoring culling metadata (`rating`, `flag`,
/// `path`, `label`) so starring or flagging a photo never creates a spurious undo step.
fn recipes_edit_equal(a: &Value, b: &Value) -> bool {
    let (Some(oa), Some(ob)) = (a.as_object(), b.as_object()) else {
        return a == b;
    };
    for (k, va) in oa {
        if IGNORED_DIFF_KEYS.contains(&k.as_str()) {
            continue;
        }
        if ob.get(k) != Some(va) {
            return false;
        }
    }
    for k in ob.keys() {
        if IGNORED_DIFF_KEYS.contains(&k.as_str()) {
            continue;
        }
        if !oa.contains_key(k) {
            return false;
        }
    }
    true
}

fn describe_diff(a: &Value, b: &Value) -> DiffDescriptor {
    let mut keys = Vec::new();
    let mut last_key: Option<&String> = None;

    if let (Some(oa), Some(ob)) = (a.as_object(), b.as_object()) {
        for (k, vb) in ob {
            if IGNORED_DIFF_KEYS.contains(&k.as_str()) {
                continue;
            }
            if oa.get(k) == Some(vb) {
                continue;
            }
            keys.push(k.clone());
            last_key = Some(k);
        }
        for k in oa.keys() {
            if IGNORED_DIFF_KEYS.contains(&k.as_str()) || ob.contains_key(k) {
                continue;
            }
            keys.push(k.clone());
            last_key = Some(k);
        }
    }

    let (single_key, single_value) = if keys.len() == 1
        && let Some(k) = last_key
        && SINGLE_NUMERIC_KEYS.contains(&k.as_str())
        && let Some(num) = b.get(k).and_then(Value::as_f64)
    {
        (Some(k.clone()), Some(num))
    } else {
        (None, None)
    };

    DiffDescriptor { kind: "diff", keys, single_key, single_value }
}

impl HistoryState {
    pub(crate) fn reset(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.baseline = None;
    }

    pub(crate) fn set_baseline(&mut self, recipe_json: &str) {
        self.baseline = parse_recipe(recipe_json);
    }

    fn push_undo_bounded(&mut self, val: Value) {
        if self.undo_stack.len() >= MAX_HISTORY {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(val);
    }

    pub(crate) fn commit_pending(&mut self, current_recipe_json: &str) -> bool {
        let Some(cur) = parse_recipe(current_recipe_json) else {
            return false;
        };
        let Some(base) = self.baseline.clone() else {
            self.baseline = Some(cur);
            return false;
        };
        if recipes_edit_equal(&cur, &base) {
            self.baseline = Some(cur);
            return false;
        }
        let same_as_top = self.undo_stack.last().is_some_and(|top| recipes_edit_equal(top, &base));
        if !same_as_top {
            self.push_undo_bounded(base);
        }
        self.redo_stack.clear();
        self.baseline = Some(cur);
        true
    }

    pub(crate) fn push_state(&mut self, current_recipe_json: &str) -> bool {
        let committed = self.commit_pending(current_recipe_json);
        let Some(cur) = parse_recipe(current_recipe_json) else {
            return committed;
        };
        let same_as_top = self.undo_stack.last().is_some_and(|top| recipes_edit_equal(top, &cur));
        let pushed = if !same_as_top {
            self.push_undo_bounded(cur.clone());
            self.redo_stack.clear();
            true
        } else {
            false
        };
        self.baseline = Some(cur);
        committed || pushed
    }

    pub(crate) fn undo(&mut self, current_recipe_json: &str) -> Option<String> {
        self.commit_pending(current_recipe_json);
        let prev = self.undo_stack.pop()?;
        if let Some(cur) = parse_recipe(current_recipe_json) {
            self.redo_stack.push(cur);
        }
        self.baseline = Some(prev.clone());
        serde_json::to_string(&prev).ok()
    }

    pub(crate) fn redo(&mut self, current_recipe_json: &str) -> Option<String> {
        self.commit_pending(current_recipe_json);
        let next = self.redo_stack.pop()?;
        if let Some(cur) = parse_recipe(current_recipe_json) {
            self.push_undo_bounded(cur);
        }
        self.baseline = Some(next.clone());
        serde_json::to_string(&next).ok()
    }

    pub(crate) fn jump_to(&mut self, index: usize, current_recipe_json: &str) -> Option<String> {
        self.commit_pending(current_recipe_json);
        let cur = parse_recipe(current_recipe_json)?;
        let current_idx = self.undo_stack.len();
        let total = current_idx + 1 + self.redo_stack.len();
        if index >= total || index == current_idx {
            return None;
        }
        let mut states = Vec::with_capacity(total);
        states.extend(self.undo_stack.iter().cloned());
        states.push(cur);
        states.extend(self.redo_stack.iter().rev().cloned());

        let target = states[index].clone();
        self.undo_stack = states[..index].to_vec();
        self.redo_stack = states[index + 1..].iter().rev().cloned().collect();
        self.baseline = Some(target.clone());
        serde_json::to_string(&target).ok()
    }

    pub(crate) fn diff_descriptors(&self, current_recipe_json: &str) -> String {
        let Some(cur) = parse_recipe(current_recipe_json) else {
            return "[]".to_string();
        };
        let mut states: Vec<&Value> = Vec::with_capacity(self.undo_stack.len() + 1 + self.redo_stack.len());
        states.extend(self.undo_stack.iter());
        states.push(&cur);
        states.extend(self.redo_stack.iter().rev());

        let mut out = Vec::with_capacity(states.len());
        for (i, state) in states.iter().enumerate() {
            if i == 0 {
                out.push(DiffDescriptor { kind: "start", keys: Vec::new(), single_key: None, single_value: None });
            } else {
                out.push(describe_diff(states[i - 1], state));
            }
        }
        serde_json::to_string(&out).unwrap_or_else(|_| "[]".to_string())
    }
}

impl qobject::HistoryController {
    fn sync_properties(mut self: Pin<&mut Self>) {
        let undo_count = self.state.undo_stack.len() as i32;
        let redo_count = self.state.redo_stack.len() as i32;
        let can_undo = undo_count > 0;
        let can_redo = redo_count > 0;

        let undo_changed = undo_count != self.undo_count;
        let redo_changed = redo_count != self.redo_count;
        let can_undo_changed = can_undo != self.can_undo;
        let can_redo_changed = can_redo != self.can_redo;

        {
            let r = self.as_mut().rust_mut().get_mut();
            r.undo_count = undo_count;
            r.redo_count = redo_count;
            r.can_undo = can_undo;
            r.can_redo = can_redo;
            r.revision = r.revision.wrapping_add(1);
        }

        if undo_changed {
            self.as_mut().undo_count_changed();
        }
        if redo_changed {
            self.as_mut().redo_count_changed();
        }
        if can_undo_changed {
            self.as_mut().can_undo_changed();
        }
        if can_redo_changed {
            self.as_mut().can_redo_changed();
        }
        self.as_mut().revision_changed();
    }

    pub fn reset(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().get_mut().state.reset();
        self.sync_properties();
    }

    pub fn set_baseline(mut self: Pin<&mut Self>, recipe_json: &QString) {
        let raw = String::from(recipe_json);
        self.as_mut().rust_mut().get_mut().state.set_baseline(&raw);
    }

    pub fn push_state(mut self: Pin<&mut Self>, current_recipe_json: &QString) {
        let raw = String::from(current_recipe_json);
        if self.as_mut().rust_mut().get_mut().state.push_state(&raw) {
            self.sync_properties();
        }
    }

    pub fn commit_pending(mut self: Pin<&mut Self>, current_recipe_json: &QString) -> bool {
        let raw = String::from(current_recipe_json);
        let changed = self.as_mut().rust_mut().get_mut().state.commit_pending(&raw);
        if changed {
            self.sync_properties();
        }
        changed
    }

    pub fn undo(mut self: Pin<&mut Self>, current_recipe_json: &QString) -> QString {
        let raw = String::from(current_recipe_json);
        let result = self.as_mut().rust_mut().get_mut().state.undo(&raw);
        self.sync_properties();
        result.map_or_else(QString::default, |s| QString::from(s.as_str()))
    }

    pub fn redo(mut self: Pin<&mut Self>, current_recipe_json: &QString) -> QString {
        let raw = String::from(current_recipe_json);
        let result = self.as_mut().rust_mut().get_mut().state.redo(&raw);
        self.sync_properties();
        result.map_or_else(QString::default, |s| QString::from(s.as_str()))
    }

    pub fn jump_to(mut self: Pin<&mut Self>, index: i32, current_recipe_json: &QString) -> QString {
        let Ok(idx) = usize::try_from(index) else {
            return QString::default();
        };
        let raw = String::from(current_recipe_json);
        let result = self.as_mut().rust_mut().get_mut().state.jump_to(idx, &raw);
        if result.is_some() {
            self.sync_properties();
        }
        result.map_or_else(QString::default, |s| QString::from(s.as_str()))
    }

    pub fn diff_descriptors(&self, current_recipe_json: &QString) -> QString {
        let raw = String::from(current_recipe_json);
        let json = self.state.diff_descriptors(&raw);
        QString::from(json.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_pending_coalesces_slider_drag_and_ignores_culling() {
        let mut h = HistoryState::default();
        h.set_baseline(r#"{"exposure":0.0,"contrast":0.0,"rating":0,"flag":"none"}"#);

        // Changing only rating/flag must not push a phantom undo entry
        assert!(!h.commit_pending(r#"{"exposure":0.0,"contrast":0.0,"rating":4,"flag":"pick"}"#));
        assert_eq!(h.undo_stack.len(), 0);

        // Committing after an exposure edit pushes the pre-edit baseline once
        assert!(h.commit_pending(r#"{"exposure":1.25,"contrast":0.0,"rating":4,"flag":"pick"}"#));
        assert_eq!(h.undo_stack.len(), 1);
        assert_eq!(h.undo_stack[0]["exposure"], 0.0);

        // Calling commit_pending again with no further change is a no-op
        assert!(!h.commit_pending(r#"{"exposure":1.25,"contrast":0.0,"rating":4,"flag":"pick"}"#));
        assert_eq!(h.undo_stack.len(), 1);
    }

    #[test]
    fn undo_redo_and_jump_to_round_trip() {
        let mut h = HistoryState::default();
        let s0 = r#"{"exposure":0.0,"contrast":0.0}"#;
        let s1 = r#"{"exposure":0.5,"contrast":0.0}"#;
        let s2 = r#"{"exposure":0.5,"contrast":20.0}"#;

        h.set_baseline(s0);
        assert!(h.commit_pending(s1));
        assert!(h.commit_pending(s2));
        assert_eq!(h.undo_stack.len(), 2);

        let desc: Value = serde_json::from_str(&h.diff_descriptors(s2)).unwrap();
        let arr = desc.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["kind"], "start");
        assert_eq!(arr[1]["single_key"], "exposure");
        assert_eq!(arr[1]["single_value"], 0.5);
        assert_eq!(arr[2]["single_key"], "contrast");
        assert_eq!(arr[2]["single_value"], 20.0);

        // Undo back to s1
        let prev = h.undo(s2).unwrap();
        let prev_val: Value = serde_json::from_str(&prev).unwrap();
        assert_eq!(prev_val["contrast"], 0.0);
        assert_eq!(prev_val["exposure"], 0.5);
        assert_eq!(h.undo_stack.len(), 1);
        assert_eq!(h.redo_stack.len(), 1);

        // Jump straight to index 0 (s0), preserving s1 and s2 on redo
        let jumped = h.jump_to(0, &prev).unwrap();
        let jumped_val: Value = serde_json::from_str(&jumped).unwrap();
        assert_eq!(jumped_val["exposure"], 0.0);
        assert_eq!(h.undo_stack.len(), 0);
        assert_eq!(h.redo_stack.len(), 2);

        // Redo restores s1
        let redone = h.redo(&jumped).unwrap();
        let redone_val: Value = serde_json::from_str(&redone).unwrap();
        assert_eq!(redone_val["exposure"], 0.5);
        assert_eq!(h.undo_stack.len(), 1);
        assert_eq!(h.redo_stack.len(), 1);
    }

    #[test]
    fn history_stack_enforces_max_capacity() {
        let mut h = HistoryState::default();
        h.set_baseline(r#"{"contrast":0}"#);
        for i in 1..=(MAX_HISTORY + 15) {
            let cur = format!(r#"{{"contrast":{i}}}"#);
            assert!(h.commit_pending(&cur));
        }
        assert_eq!(h.undo_stack.len(), MAX_HISTORY);
        // Oldest entries were evicted, keeping the most recent MAX_HISTORY baselines
        assert_eq!(h.undo_stack[0]["contrast"], 15);
    }
}
