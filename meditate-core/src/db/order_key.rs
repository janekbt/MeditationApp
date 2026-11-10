//! Sort keys that order a synced list as plain text (presets and
//! guided files keep them in `created_iso`, which older versions
//! already sort by). A move rewrites only the moved row's key.

// ponytail: moves dropped again and again at the same edge of one gap
// add a digit every 5 to 10 moves (about 160 characters after 1000);
// count in the neighbour's tail, as count_up does, if that ever shows.
/// A `created_iso` that sorts as text strictly between `before` and
/// `after` (`None`: the list's end). Between two neighbours it extends
/// `before` with digits, so there is always room; a tail from the
/// moved preset's uuid keeps two devices' moves into one gap apart.
/// `None` only for neighbours with equal text, or a first preset with
/// no digit above 0.
pub(crate) fn key_between(before: Option<&str>, after: Option<&str>, uuid: &str) -> Option<String> {
    let core = match (before, after) {
        (Some(lo), Some(hi)) if lo < hi => match hi.strip_prefix(lo) {
            Some(rest) => format!("{lo}{}", digits_below(rest)?),
            None => just_above(lo, hi),
        },
        // The first digit above 0 lowered: below `hi` for any text. The
        // 9 after it gives the next moves to the top room to lower.
        (None, Some(hi)) => {
            let j = hi.bytes().position(|b| (b'1'..=b'9').contains(&b))?;
            format!("{}{}9", &hi[..j], char::from(hi.as_bytes()[j] - 1))
        }
        // Now, so a preset saved later still comes after it. With the
        // clock behind `lo`: `lo` without its tail, counted up by one in
        // its last digits, so it stays as long and as early as it is.
        (Some(lo), None) => {
            let now = chrono::Utc::now().to_rfc3339();
            if now.as_str() > lo { now } else { count_up(lo) }
        }
        _ => return None,
    };
    // The last 8 hex digits (the bundled ids share their first ones),
    // each as two digits 1-9: never ending in 0, so digits_below always
    // finds room, and two ids never give one tail.
    let tail: String = uuid.chars().rev().filter_map(|c| c.to_digit(16)).take(8)
        .flat_map(|v| [char::from(b'1' + (v / 9) as u8), char::from(b'1' + (v % 9) as u8)]).collect();
    let key = core + &tail;
    (before.map_or(true, |lo| lo < key.as_str()) && after.map_or(true, |hi| key.as_str() < hi)).then_some(key)
}

/// `n` fresh keys from now, 1 ms apart, in order: the fallback when a
/// list has no room left between neighbours.
pub(crate) fn fresh_keys(n: usize) -> Vec<String> {
    let start = chrono::Utc::now();
    (0..n)
        .map(|i| (start + chrono::Duration::milliseconds(i as i64)).to_rfc3339_opts(chrono::SecondsFormat::Millis, false))
        .collect()
}

/// The shortest start of `lo`, with one digit raised, that sorts above
/// `lo` and below `hi`, where `hi` doesn't start with `lo`; so moves
/// back and forth don't grow the text.
fn just_above(lo: &str, hi: &str) -> String {
    let (l, h) = (lo.as_bytes(), hi.as_bytes());
    let split = l.iter().zip(h).position(|(a, b)| a != b).unwrap_or(l.len());
    (split..l.len())
        .find_map(|j| {
            let digit = l[j].saturating_add(1).max(b'1');
            let start = lo.get(..j).filter(|_| digit <= b'9' && (j > split || digit < h[j]))?;
            Some(format!("{start}{}", char::from(digit)))
        })
        .unwrap_or_else(|| format!("{lo}5"))
}

/// The next text above `lo`: its last digit below 9 raised and the
/// digits after it set to 0. The count runs in a move's 16-digit tail
/// (or a plain time's offset), never in the time before it, so the
/// place stays as early as it is; a second tail on top is dropped
/// first, so the text stays as long. Every tail holds a digit below 9.
fn count_up(lo: &str) -> String {
    let n = lo.len();
    let core = if n >= 32 && lo.as_bytes()[n - 32..].iter().all(u8::is_ascii_digit) { &lo[..n - 16] } else { lo };
    let mut bytes = core.as_bytes().to_vec();
    let Some(j) = bytes.iter().rposition(|b| (b'0'..=b'8').contains(b)) else { return format!("{lo}5"); };
    bytes[j] += 1;
    bytes[j + 1..].iter_mut().filter(|b| b.is_ascii_digit()).for_each(|b| *b = b'0');
    String::from_utf8(bytes).expect("only ASCII digits changed")
}

/// A digit string that sorts below `rest` and is not a start of it;
/// `rest` is digits not ending in 0, as `key_between` writes them.
fn digits_below(rest: &str) -> Option<String> {
    let zeros = rest.bytes().take_while(|&b| b == b'0').count();
    let low = match *rest.as_bytes().get(zeros)? {
        first @ b'2'..=b'9' => char::from(first - 1).to_string(),
        b'1' => "05".into(),
        _ => return None,
    };
    Some(format!("{}{low}", &rest[..zeros]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_keys_are_ascending_and_one_per_item() {
        let keys = fresh_keys(3);
        assert_eq!(keys.len(), 3);
        assert!(keys[0] < keys[1] && keys[1] < keys[2], "{keys:?}");
        assert_eq!(keys[0].len(), "2026-11-10T12:00:00.000+00:00".len(), "{keys:?}");
        assert!(fresh_keys(0).is_empty());
    }
}
