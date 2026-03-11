use cba::bring::consume_escaped;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Substitute characters present as keys in the map, unless they are escaped.
pub fn substitute_escaped<U: AsRef<str>>(input: &str, map: &[(char, U)]) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some(&k) => {
                    if let Some((_, replacement)) = map.iter().find(|(key, _)| *key == k) {
                        out.push_str(replacement.as_ref());
                        chars.next();
                    } else {
                        out.push('\\');
                        out.push(k);
                        chars.next();
                    }
                }

                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }

    out
}

pub fn fit_width(input: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;

    for g in input.graphemes(true) {
        let g_width = UnicodeWidthStr::width(g);

        if used + g_width > width {
            break;
        }

        out.push_str(g);
        used += g_width;
    }

    // Pad if needed
    if used < width {
        out.extend(std::iter::repeat(' ').take(width - used));
    }

    out
}

/// Resolve escape sequences
pub fn resolve_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            consume_escaped(&mut chars, &mut out);
            continue;
        }
        out.push(c);
    }
    out
}
/// Allocates widths to a constrained available space while preserving order relations,
/// ignoring zero-widths, and enforcing a minimum width floor.
///
/// Constraints:
/// 1. **Idempotence**: If the original widths fit AND satisfy `min_width`, they are returned as-is.
/// 2. **Zero-Exclusion**: Original widths of `0` remain `0` and do not consume space or affect order.
/// 3. **Minimum Floor**: No non-zero width can be less than `min_width`.
/// 4. **Order Preservation**: Strict and non-strict inequalities hold true for all non-zero elements.
/// 5. **Greedy Preference**: Earlier elements are kept as close to their original bounds as possible.
///
/// # Returns
/// * `Ok(Vec<u16>)` - The newly allocated widths that fit within `total_available_space`.
/// * `Err(Vec<u16>)` - The absolute minimal allocation possible that satisfies all relative
///   and minimum-width constraints, returned if it exceeds `total_available_space`.
/// Allocates widths to a constrained available space while preserving order relations,
/// ignoring zero-widths, and enforcing a minimum width floor.
pub fn allocate_widths(
    widths: &[u16],
    total_available_space: u16,
    min_width: u16,
) -> Result<Vec<u16>, Vec<u16>> {
    let current_sum: u16 = widths.iter().sum();
    let already_meets_min = widths.iter().all(|&w| w == 0 || w >= min_width);
    if already_meets_min && current_sum <= total_available_space || current_sum == 0 {
        return Ok(widths.to_vec());
    }

    let mut unique_widths: Vec<u16> = widths.iter().copied().filter(|&w| w > 0).collect();
    unique_widths.sort_unstable();
    unique_widths.dedup();
    let k = unique_widths.len();

    let mut counts = vec![0; k];
    for &w in widths {
        if w > 0 {
            let idx = unique_widths.binary_search(&w).unwrap();
            counts[idx] += 1;
        }
    }

    let min_possible_sum: u16 = counts
        .iter()
        .enumerate()
        .map(|(j, &c)| c * (min_width + j as u16))
        .sum();

    if total_available_space < min_possible_sum {
        let minimal_allocation = widths
            .iter()
            .map(|&w| {
                if w == 0 {
                    0
                } else {
                    let idx = unique_widths.binary_search(&w).unwrap();
                    min_width + idx as u16
                }
            })
            .collect();
        return Err(minimal_allocation);
    }

    let mut order_to_fix = Vec::with_capacity(k);
    let mut seen = vec![false; k];
    for &w in widths {
        if w > 0 {
            let idx = unique_widths.binary_search(&w).unwrap();
            if !seen[idx] {
                seen[idx] = true;
                order_to_fix.push(idx);
            }
        }
    }

    let mut fixed_vars: Vec<Option<u16>> = vec![None; k];

    // Refactored helper to safely derive current absolute bounds
    let get_bounds = |fixed_state: &[Option<u16>]| -> (Vec<u16>, Vec<u16>) {
        let mut l: Vec<u16> = (0..k).map(|j| min_width + j as u16).collect();
        let mut u: Vec<u16> = unique_widths
            .iter()
            .enumerate()
            .map(|(j, &w)| w.max(min_width + j as u16))
            .collect();

        for j in 0..k {
            if let Some(val) = fixed_state[j] {
                l[j] = val;
                u[j] = val;
            }
        }

        for j in 1..k {
            if l[j] < l[j - 1] + 1 {
                l[j] = l[j - 1] + 1;
            }
        }

        for j in (0..k.saturating_sub(1)).rev() {
            if u[j] > u[j + 1].saturating_sub(1) {
                u[j] = u[j + 1].saturating_sub(1);
            }
        }
        (l, u)
    };

    let is_valid = |fixed_state: &[Option<u16>]| -> bool {
        let (l, u) = get_bounds(fixed_state);
        let mut min_sum = 0;
        for j in 0..k {
            if l[j] > u[j] {
                return false;
            }
            min_sum += counts[j] * l[j];
        }
        min_sum <= total_available_space
    };

    // 4. Greedy Maximization
    for &m in &order_to_fix {
        // Calculate the absolute tightest bounds based on currently fixed variables
        let (current_l, current_u) = get_bounds(&fixed_vars);

        let mut low = current_l[m];
        let mut high = current_u[m];
        let mut best_v = low;

        while low <= high {
            let mid = low + (high - low) / 2;
            fixed_vars[m] = Some(mid);

            if is_valid(&fixed_vars) {
                best_v = mid;
                low = mid + 1;
            } else {
                high = mid - 1;
                fixed_vars[m] = None;
            }
        }

        fixed_vars[m] = Some(best_v);
    }

    let result = widths
        .iter()
        .map(|&w| {
            if w == 0 {
                0
            } else {
                let idx = unique_widths.binary_search(&w).unwrap();
                fixed_vars[idx].unwrap()
            }
        })
        .collect();

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zeros_are_ignored() {
        let original = vec![0, 100, 0, 50, 0];
        let space = 100;
        let min_width = 10;
        // 100 -> max possible, 50 -> must be smaller than 100.
        // 100 gets priority. 50 must be at least 10. So 100 -> 90, 50 -> 10.
        let result = allocate_widths(&original, space, min_width).unwrap();
        assert_eq!(result, vec![0, 90, 0, 10, 0]);
    }

    #[test]
    fn test_min_width_floor_bumping() {
        // Elements start out smaller than the min_width
        let original = vec![2, 4, 6];
        let space = 50;
        let min_width = 10;

        // Since space is abundant, they grow up to the minimum valid bounds that satisfy min_width
        // 2 -> 10
        // 4 -> 11
        // 6 -> 12
        let result = allocate_widths(&original, space, min_width).unwrap();
        assert_eq!(result, vec![10, 11, 12]);
    }

    #[test]
    fn test_strict_inequality_with_min_width() {
        let original = vec![50, 60, 70];
        let min_width = 20;
        // The absolute minimum space required is 20 + 21 + 22 = 63
        let result = allocate_widths(&original, 63, min_width).unwrap();
        assert_eq!(result, vec![20, 21, 22]);

        let err_result = allocate_widths(&original, 62, min_width);
        assert!(err_result.is_err());
    }

    #[test]
    fn test_idempotence_with_min_width() {
        // Fits perfectly in space (300) AND all elements >= min_width (50)
        let original_valid = vec![100, 100, 100];
        assert_eq!(
            allocate_widths(&original_valid, 300, 50).unwrap(),
            vec![100, 100, 100]
        );

        // Fits perfectly in space (60), but violates min_width (50)
        let original_invalid = vec![20, 20, 20];
        // Must bump to min_width, failing because 50+50+50 > 60
        assert!(allocate_widths(&original_invalid, 60, 50).is_err());
    }

    #[test]
    fn test_returns_minimal_allocation_on_err() {
        let original = vec![50, 60, 70];
        let min_width = 20;
        // The minimal valid allocation for this is [20, 21, 22].
        // This requires a sum of 63.

        // If we provide exactly 63, it returns Ok
        let ok_result = allocate_widths(&original, 63, min_width);
        assert_eq!(ok_result, Ok(vec![20, 21, 22]));

        let ok_result = allocate_widths(&original, 70, min_width);
        assert_eq!(ok_result, Ok(vec![22, 23, 25]));

        // If we provide less than 63, it should return the exact same array, but as an Err
        let err_result = allocate_widths(&original, 50, min_width);
        assert_eq!(err_result, Err(vec![20, 21, 22]));
    }

    #[test]
    fn test_minimal_allocation_handles_zeros_and_duplicates() {
        let original = vec![0, 100, 100, 50, 0, 200];
        let min_width = 10;
        let space = 10; // Extremely constrained

        // Unique non-zeros sorted: 50, 100, 200
        // Minimal mapping:
        // 50 -> 10 + 0 = 10
        // 100 -> 10 + 1 = 11
        // 200 -> 10 + 2 = 12
        //
        // Final expected minimal array: [0, 11, 11, 10, 0, 12]
        // Required sum = 11 + 11 + 10 + 12 = 44

        let err_result = allocate_widths(&original, space, min_width);
        assert_eq!(err_result, Err(vec![0, 11, 11, 10, 0, 12]));
    }
}
