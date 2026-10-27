use std::collections::BTreeSet;
#[derive(Default)]
pub struct Selection {
    pub selected: BTreeSet<usize>,
    pub focus: Option<usize>,
    pub anchor: Option<usize>,
}
impl Selection {
    pub fn choose(&mut self, index: usize, ctrl: bool, shift: bool, visible: &[usize]) {
        if shift {
            let anchor = self.anchor.unwrap_or(index);
            let a = visible.iter().position(|i| *i == anchor).unwrap_or(0);
            let b = visible.iter().position(|i| *i == index).unwrap_or(a);
            if !ctrl {
                self.selected.clear();
            }
            self.selected
                .extend(visible[a.min(b)..=a.max(b)].iter().copied());
        } else {
            if !ctrl {
                self.selected.clear();
            }
            if !self.selected.insert(index) && ctrl {
                self.selected.remove(&index);
            }
            self.anchor = Some(index);
        }
        self.focus = Some(index);
    }
    pub fn step(&mut self, delta: isize, ctrl: bool, shift: bool, visible: &[usize]) {
        if visible.is_empty() {
            return;
        }
        let at = self
            .focus
            .and_then(|f| visible.iter().position(|i| *i == f))
            .unwrap_or(0);
        let index = visible[(at as isize + delta).clamp(0, visible.len() as isize - 1) as usize];
        if ctrl && !shift {
            self.focus = Some(index);
        } else {
            self.choose(index, ctrl, shift, visible);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges_and_additive_ranges() {
        let mut s = Selection::default();
        let v = vec![0, 1, 2, 3, 4];
        s.choose(1, false, false, &v);
        s.choose(3, false, true, &v);
        assert_eq!(s.selected, BTreeSet::from([1, 2, 3]));
        s.choose(0, true, false, &v);
        s.choose(1, true, true, &v);
        assert_eq!(s.selected, BTreeSet::from([0, 1, 2, 3]));
    }
    #[test]
    fn control_arrows_only_move_focus() {
        let mut s = Selection::default();
        let v = vec![0, 1, 2];
        s.choose(0, false, false, &v);
        s.step(1, true, false, &v);
        assert_eq!(s.focus, Some(1));
        assert_eq!(s.selected, BTreeSet::from([0]));
        s.step(1, false, true, &v);
        assert_eq!(s.selected.len(), 3);
    }
}
