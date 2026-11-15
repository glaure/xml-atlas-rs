//! The open documents, one tab each, and which one is shown (`docs/spec/open.md`, OPEN-5).

/// Tabs in the order they were opened, and the one shown.
pub struct Tabs<T> {
    items: Vec<T>,
    active: usize,
}

impl<T> Default for Tabs<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            active: 0,
        }
    }
}

impl<T> Tabs<T> {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&T> {
        self.items.get(i)
    }

    pub fn get_mut(&mut self, i: usize) -> Option<&mut T> {
        self.items.get_mut(i)
    }

    /// Index of the tab shown; meaningless when there are none.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// The tab shown.
    pub fn active(&self) -> Option<&T> {
        self.items.get(self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut T> {
        self.items.get_mut(self.active)
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.items.iter_mut()
    }

    /// The first tab for which `f` is true.
    pub fn position(&self, f: impl FnMut(&T) -> bool) -> Option<usize> {
        self.items.iter().position(f)
    }

    /// Adds a tab after the others and shows it.
    pub fn push(&mut self, item: T) {
        self.items.push(item);
        self.active = self.items.len() - 1;
    }

    /// Shows tab `i`, if there is one.
    pub fn select(&mut self, i: usize) {
        if i < self.items.len() {
            self.active = i;
        }
    }

    /// Shows the next tab, after the last the first; `back` the one before.
    pub fn cycle(&mut self, back: bool) {
        let n = self.items.len();
        if n > 0 {
            self.active = if back {
                (self.active + n - 1) % n
            } else {
                (self.active + 1) % n
            };
        }
    }

    /// Removes tab `i`. Closing the tab shown shows the one after it, or before it when it was
    /// the last; closing another keeps the same tab shown.
    pub fn close(&mut self, i: usize) -> Option<T> {
        if i >= self.items.len() {
            return None;
        }
        let item = self.items.remove(i);
        if i < self.active || self.active >= self.items.len() {
            self.active = self.active.saturating_sub(1);
        }
        Some(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(names: &[&'static str]) -> Tabs<&'static str> {
        let mut t = Tabs::default();
        for n in names {
            t.push(*n);
        }
        t
    }

    #[test]
    fn a_new_tab_is_shown() {
        let mut t = tabs(&["a.xml", "b.xml"]);
        assert_eq!(t.active(), Some(&"b.xml"));
        t.select(0);
        t.push("ü.xml");
        assert_eq!((t.active_index(), t.active()), (2, Some(&"ü.xml")));
        t.select(7);
        assert_eq!(t.active_index(), 2);
    }

    #[test]
    fn closing_keeps_a_sensible_tab_shown() {
        let mut t = tabs(&["a", "b", "c", "d"]);
        t.select(1);
        // Closing the tab shown shows the one after it.
        assert_eq!(t.close(1), Some("b"));
        assert_eq!(t.active(), Some(&"c"));
        // Closing a tab before the one shown keeps it shown.
        t.close(0);
        assert_eq!(t.active(), Some(&"c"));
        // Closing one after it, too.
        t.close(1);
        assert_eq!(t.active(), Some(&"c"));
        t.push("e");
        // The last tab shown: the one before it follows.
        t.close(1);
        assert_eq!(t.active(), Some(&"c"));
        t.close(0);
        assert!(t.is_empty());
        assert_eq!(t.active(), None);
        assert_eq!(t.close(0), None);
    }

    #[test]
    fn cycle_wraps_around() {
        let mut t = tabs(&["a", "b", "c"]);
        t.cycle(false);
        assert_eq!(t.active(), Some(&"a"));
        t.cycle(true);
        assert_eq!(t.active(), Some(&"c"));
        t.cycle(true);
        assert_eq!(t.active(), Some(&"b"));
        let mut empty: Tabs<&str> = Tabs::default();
        empty.cycle(false);
        assert_eq!(empty.active(), None);
    }
}
