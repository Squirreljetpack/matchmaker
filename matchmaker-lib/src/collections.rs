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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
        if n >= 64 { !0 } else { (1u64 << n) - 1 }
    }
}
