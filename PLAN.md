# Rework make_table in results.rs

1. Change worker api to row based 
2. Change max_widths determination to fixed
3. Convert make_table to use the new worker row api

# 0
rename results to get_row (no more snapshot.matched_items): use snapshot.get_matched_item(n) to fetch the item


# 1
Currently max_widths is based on median_widths. We want to change to a fixed column width + resizable approach. Here's how we do it.

# Steps
Fix rendering
Fix downloading lfs