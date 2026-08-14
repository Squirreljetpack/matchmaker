use cba::define_collection_wrapper;

define_collection_wrapper!(
  /// A set of nucleo `u32` indices representing the items the user has selected.
  ///
  /// The index is the nucleo item index (the value stored in [`nucleo::Match::idx`])
  /// and is stable for the lifetime of the worker's items. It is used as the row-cache
  /// key in `ResultsUI` so that selected rows can be highlighted.
  #[derive(Debug)]
  Selector : indexmap::IndexSet<u32>
);

impl Selector {
    pub fn cycle_all_bg(&mut self, indices: impl ExactSizeIterator<Item = u32> + Clone) {
        let len = indices.len();

        // check if indices is a subset of self
        if len <= self.len() {
            let mut cloned_indices = indices.clone();

            if cloned_indices.all(|item| self.contains(&item)) {
                self.clear();
                return;
            }
        }

        self.reserve(len);
        self.extend(indices);
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiddenColumns {
    /// Bitfield where bit `i` is 1 if hidden, 0 if visible.
    mask: u64,
    /// LIFO insertion order stack for hidden column indices.
    order: Vec<u8>,
    /// Number of tracked columns (0..=64).
    len: u8,
}

impl HiddenColumns {
    /// Creates a `HiddenColumns` with a fixed mask size (up to 64 columns), all initially visible.
    ///
    /// # Panics
    /// Panics if `size > 64`.
    pub fn new_with_size(size: usize) -> Self {
        assert!(size <= 64, "HiddenColumns size cannot exceed 64");
        Self {
            mask: 0,
            order: Vec::with_capacity(size),
            len: size as u8,
        }
    }

    /// Resizes the number of tracked columns, preserving the hidden state of
    /// columns that remain in range. Growing adds visible columns; truncating
    /// drops hidden flags and order entries beyond the new size. The size is
    /// silently clamped to 64.
    pub fn resize(&mut self, new_size: usize) {
        let new_size = new_size.min(64) as u8;
        if new_size == self.len {
            return;
        }

        if new_size < self.len {
            // new_size <= 63 here, so the shift cannot overflow.
            self.mask &= (1u64 << new_size) - 1;
            self.order.retain(|&i| (i as usize) < new_size as usize);
        }

        self.len = new_size;
    }

    #[inline]
    pub fn mask_len(&self) -> usize {
        self.len as usize
    }

    /// Returns the raw 64-bit visibility bitfield (`1` = hidden, `0` = visible).
    #[inline]
    pub fn mask(&self) -> Vec<bool> {
        (0..self.len)
            .map(|i| (self.mask & (1u64 << i)) != 0)
            .collect()
    }

    /// Return the number of visible (non-hidden) columns.
    #[inline]
    pub fn visible_count(&self) -> usize {
        (!self.mask & self.valid_mask()).count_ones() as usize
    }

    /// Iterator over `(index, hidden)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (usize, bool)> + '_ {
        (0..self.len as usize).map(move |i| (i, (self.mask & (1u64 << i)) != 0))
    }

    /// Checks if column `value` is hidden.
    /// Returns false if out of bounds.
    #[inline]
    pub fn contains(&self, value: usize) -> bool {
        if value >= self.len as usize {
            false
        } else {
            (self.mask & (1u64 << value)) != 0
        }
    }

    /// Hides column `i` and adds it to the order stack.
    /// Does nothing if out of bounds or already hidden.
    pub fn set(&mut self, i: usize) {
        if i >= self.len as usize || self.contains(i) {
            return;
        }

        self.mask |= 1u64 << i;
        self.order.push(i as u8);
    }

    /// Unhides column `i` and removes it from the order stack.
    /// Does nothing if out of bounds or already visible.
    pub fn unset(&mut self, i: usize) {
        if i >= self.len as usize || !self.contains(i) {
            return;
        }

        self.mask &= !(1u64 << i);
        if let Some(pos) = self.order.iter().position(|&x| x == i as u8) {
            self.order.remove(pos);
        }
    }

    /// Unhides the last inserted element and updates the mask.
    pub fn pop(&mut self) -> Option<usize> {
        let value = self.order.pop()? as usize;
        self.mask &= !(1u64 << value);
        Some(value)
    }

    /// Clears all hidden state, making all columns visible.
    pub fn clear(&mut self) {
        self.order.clear();
        self.mask = 0;
    }

    /// Returns the first visible index >= `n`.
    pub fn next_gap(&self, n: usize) -> usize {
        if n >= self.len as usize {
            return n;
        }

        let n_and_above = !0u64 << n;
        let available = !self.mask & self.valid_mask() & n_and_above;

        if available == 0 {
            self.len as usize
        } else {
            available.trailing_zeros() as usize
        }
    }

    /// Returns the first visible index < `n`.
    pub fn prev_gap(&self, n: usize) -> Option<usize> {
        if n == 0 {
            return None;
        }
        if n > self.len as usize {
            return Some(n - 1);
        }

        let strictly_below = Self::mask_below(n);
        let available = !self.mask & self.valid_mask() & strictly_below;

        if available == 0 {
            None
        } else {
            Some(63 - available.leading_zeros() as usize)
        }
    }

    /// Like [`Self::next_gap`], but wraps around to index 0.
    pub fn next_gap_wrapping(&self, n: usize) -> usize {
        let candidate = self.next_gap(n);
        if candidate < self.len as usize {
            candidate
        } else {
            self.next_gap(0)
        }
    }

    /// Like [`Self::prev_gap`], but wraps around to the end of the mask.
    pub fn prev_gap_wrapping(&self, n: usize) -> Option<usize> {
        match self.prev_gap(n) {
            Some(idx) if idx < self.len as usize => Some(idx),
            _ => self.prev_gap(self.len as usize),
        }
    }

    /// Returns the zero-indexed `k`-th visible column index.
    pub fn nth_gap(&self, k: usize) -> usize {
        let visible = !self.mask & self.valid_mask();
        let visible_in_mask = visible.count_ones() as usize;

        if k < visible_in_mask {
            let mut val = visible;
            for _ in 0..k {
                val &= val - 1; // Clears the lowest set bit (Kernighan's Algorithm)
            }
            val.trailing_zeros() as usize
        } else {
            let remaining = k - visible_in_mask;
            self.len as usize + remaining
        }
    }

    /// If `x` is visible, returns how many visible columns exist before `x`.
    pub fn gap_index(&self, x: usize) -> Option<usize> {
        if self.contains(x) {
            return None;
        }

        if x <= self.len as usize {
            let visible_below = !self.mask & self.valid_mask() & Self::mask_below(x);
            Some(visible_below.count_ones() as usize)
        } else {
            let visible_in_mask = (!self.mask & self.valid_mask()).count_ones() as usize;
            Some(visible_in_mask + (x - self.len as usize))
        }
    }

    // --- Internal Bitmask Helpers ---

    #[inline]
    fn valid_mask(&self) -> u64 {
        if self.len == 64 {
            !0
        } else {
            (1u64 << self.len) - 1
        }
    }

    #[inline]
    fn mask_below(n: usize) -> u64 {
        if n >= 64 {
            !0
        } else {
            (1u64 << n) - 1
        }
    }
}

impl Default for HiddenColumns {
    fn default() -> Self {
        Self::new_with_size(3)
    }
}

impl FromIterator<usize> for HiddenColumns {
    /// Builds a `HiddenColumns` at max capacity (64 columns) with the given
    /// column indices hidden, in sequence.
    ///
    /// # Panics
    /// Panics if any index is out of range (`>= 64`).
    fn from_iter<T: IntoIterator<Item = usize>>(iter: T) -> Self {
        let mut columns = Self::new_with_size(64);
        for i in iter {
            assert!(
                i < 64,
                "HiddenColumns index {i} is out of range (must be < 64)"
            );
            columns.set(i);
        }
        columns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_new_with_size_3() {
        assert_eq!(HiddenColumns::default(), HiddenColumns::new_with_size(3));
    }

    #[test]
    fn resize_grows_and_truncates() {
        let mut hc = HiddenColumns::new_with_size(4);
        hc.set(1);
        hc.set(3);
        hc.pop();
        hc.set(3);

        hc.resize(2);
        assert_eq!(hc.mask_len(), 2);
        assert_eq!(hc.mask(), vec![false, true]);

        hc.resize(5);
        assert_eq!(hc.mask_len(), 5);
        assert_eq!(hc.mask(), vec![false, true, false, false, false]);
        // In-range hidden column survives a grow-truncate round trip.
        hc.resize(2);
        assert_eq!(hc.mask(), vec![false, true]);
    }

    #[test]
    fn resize_noop_on_same_size() {
        let mut hc = HiddenColumns::new_with_size(3);
        hc.set(2);
        hc.resize(3);
        assert_eq!(hc.mask_len(), 3);
        assert_eq!(hc.mask(), vec![false, false, true]);
    }

    #[test]
    fn resize_clamps_to_64() {
        let mut hc = HiddenColumns::new_with_size(1);
        hc.resize(100);
        assert_eq!(hc.mask_len(), 64);
    }

    #[test]
    fn from_iter_builds_at_max_capacity() {
        let hc = HiddenColumns::from_iter([0, 2, 63]);
        assert_eq!(hc.mask_len(), 64);
        assert!(hc.contains(0));
        assert!(hc.contains(2));
        assert!(hc.contains(63));
        assert!(!hc.contains(1));
        assert_eq!(hc.visible_count(), 61);
    }

    #[test]
    fn from_iter_preserves_push_order() {
        let mut hc = HiddenColumns::from_iter([3, 1]);
        assert_eq!(hc.pop(), Some(1));
        assert_eq!(hc.pop(), Some(3));
    }

    #[test]
    fn from_iter_collect() {
        let hc: HiddenColumns = vec![2, 5].into_iter().collect();
        assert!(hc.contains(2));
        assert!(hc.contains(5));
        assert_eq!(hc.mask_len(), 64);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn from_iter_panics_on_index_64() {
        let _ = HiddenColumns::from_iter([64]);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn from_iter_panics_on_index_over_64() {
        let _ = HiddenColumns::from_iter([1, 65]);
    }
}
