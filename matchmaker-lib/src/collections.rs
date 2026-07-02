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
    pub fn cycle_all_bg(&mut self, indices: impl ExactSizeIterator<Item = u32>) {
        let matched: indexmap::IndexSet<u32> = indices.collect();
        if !matched.is_empty() && matched.is_subset(&self.0) {
            self.0.clear();
        } else {
            self.0.extend(matched);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HiddenColumns {
    order: Vec<usize>,
    mask: Vec<bool>,
}

impl HiddenColumns {
    /// Create a `HiddenColumns` with a fixed mask of `size` columns, all initially visible.
    /// The mask size is fixed for the lifetime of the value; out-of-range indices are treated
    /// as visible and `set` is a no-op past `mask_len`.
    pub fn new_with_size(size: usize) -> Self {
        Self {
            order: Vec::new(),
            mask: vec![false; size],
        }
    }

    pub fn mask_len(&self) -> usize {
        self.mask.len()
    }

    /// Returns a slice of the underlying visibility mask. `true` means hidden.
    pub fn mask(&self) -> &[bool] {
        &self.mask
    }

    /// Set the visibility of `idx`.
    pub fn set(&mut self, i: usize, hidden: bool) {
        if i >= self.mask.len() {
            return;
        }
        if self.mask[i] == hidden {
            return;
        }
        self.mask[i] = hidden;
        if hidden {
            self.order.push(i);
        } else {
            if let Some(pos) = self.order.iter().position(|&x| x == i) {
                self.order.remove(pos);
            }
        }
    }

    /// O(N) - Returns the number of visible (non-hidden) columns.
    pub fn visible_count(&self) -> usize {
        self.mask.iter().filter(|x| !**x).count()
    }

    /// Iterator over `(index, hidden)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (usize, bool)> + '_ {
        self.mask.iter().copied().enumerate()
    }

    /// Checks if the value is in the set.
    /// Returns false if the value is out of bounds of the mask.
    pub fn contains(&self, value: usize) -> bool {
        self.mask.get(value).copied().unwrap_or(false)
    }

    /// Pushes a value onto the end of the order list if not present.
    /// Returns true if successfully inserted.
    /// Returns false if the value is out of bounds or already present.
    pub fn push(&mut self, value: usize) -> bool {
        if value >= self.mask.len() || self.contains(value) {
            false
        } else {
            self.mask[value] = true;
            self.order.push(value);
            true
        }
    }

    /// Removes the last inserted element and updates the mask.
    pub fn pop(&mut self) -> Option<usize> {
        if let Some(value) = self.order.pop() {
            self.mask[value] = false;
            Some(value)
        } else {
            None
        }
    }

    pub fn clear(&mut self) {
        self.order.clear();
        // Resetting the mask to all false is faster than re-allocating
        self.mask.fill(false);
    }

    /// O(1) amortized / O(M) worst case - First value >= n not contained in the set.
    pub fn next_gap(&self, mut n: usize) -> usize {
        // If n is outside our tracked mask, then n itself is a gap.
        while n < self.mask.len() {
            if !self.mask[n] {
                return n;
            }
            n += 1;
        }
        n
    }

    /// O(N) worst case - First value < n not contained in the set.
    pub fn prev_gap(&self, n: usize) -> Option<usize> {
        if n == 0 {
            return None;
        }

        let mut i = n - 1;
        loop {
            // Anything beyond the mask bounds is technically a gap,
            // but since i starts at n - 1, we check if it's out of bounds or false.
            if i >= self.mask.len() || !self.mask[i] {
                return Some(i);
            }

            if i == 0 {
                return None;
            }
            i -= 1;
        }
    }

    /// O(M) - Like [`Self::next_gap`], but wraps around to 0 when the
    /// direct search runs past the end of the mask. Returns the first
    /// value >= n not contained in the set, or the first gap starting
    /// from 0 if no gap exists in [n, mask_len()).
    pub fn next_gap_wrapping(&self, n: usize) -> usize {
        let candidate = self.next_gap(n);
        if candidate < self.mask.len() {
            candidate
        } else {
            self.next_gap(0)
        }
    }

    /// O(M) - Like [`Self::prev_gap`], but wraps around to the end of the
    /// mask when the direct search yields None or exceeds the bound.
    /// Returns the first value < n not contained in the set, or the last
    /// gap before mask_len() if no gap exists in [0, n).
    pub fn prev_gap_wrapping(&self, n: usize) -> Option<usize> {
        match self.prev_gap(n) {
            Some(idx) if idx < self.mask.len() => Some(idx),
            _ => self.prev_gap(self.mask.len()),
        }
    }

    /// O(M) where M is the mask size - Returns the k-th number NOT in the set.
    pub fn nth_gap(&self, k: usize) -> usize {
        let mut count = 0;

        // Step 1: Scan gaps inside the mask boundary
        for (n, &present) in self.mask.iter().enumerate() {
            if !present {
                if count == k {
                    return n;
                }
                count += 1;
            }
        }

        // Step 2: If k is larger than the gaps inside the mask,
        // the remaining gaps are just the numbers following the mask.
        let remaining = k - count;
        self.mask.len() + remaining
    }

    /// O(x) - If x is NOT in the set, returns how many gaps are < x.
    pub fn gap_index(&self, x: usize) -> Option<usize> {
        if self.contains(x) {
            return None;
        }

        let mut count = 0;

        // Count gaps up to x within the mask bounds
        let upper_limit = x.min(self.mask.len());
        for i in 0..upper_limit {
            if !self.mask[i] {
                count += 1;
            }
        }

        // If x is past the mask size, every element between mask.len() and x is also a gap
        if x > self.mask.len() {
            count += x - self.mask.len();
        }

        Some(count)
    }
}
