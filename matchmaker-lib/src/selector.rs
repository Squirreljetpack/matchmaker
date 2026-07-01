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
