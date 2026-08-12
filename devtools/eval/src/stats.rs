//! The two pieces of arithmetic every figure in this harness depends on.

/// A proportion with a 95% confidence interval.
///
/// **Wilson's interval, not the textbook one.** The normal approximation
/// `p ± 1.96·√(p(1-p)/n)` breaks down exactly where this harness lives: at
/// accuracies near 1. At 99.8% correct it happily reports an upper bound above
/// 100%, which is not a confidence interval, it is a bug with decimal places.
/// Wilson's interval stays inside [0, 1] by construction and is the standard
/// choice for proportions this close to the boundary.
#[derive(Clone, Copy)]
pub struct Proportion {
    pub hits: usize,
    pub total: usize,
}

impl Proportion {
    pub fn new(hits: usize, total: usize) -> Self {
        Proportion { hits, total }
    }

    pub fn rate(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        self.hits as f64 / self.total as f64
    }

    /// Wilson score interval at 95%.
    pub fn interval(&self) -> (f64, f64) {
        if self.total == 0 {
            return (0.0, 0.0);
        }
        const Z: f64 = 1.959_963_984_540_054; // two-sided 95%
        let n = self.total as f64;
        let p = self.rate();
        let denom = 1.0 + Z * Z / n;
        let centre = p + Z * Z / (2.0 * n);
        let spread = Z * ((p * (1.0 - p) / n) + (Z * Z / (4.0 * n * n))).sqrt();
        (
            ((centre - spread) / denom).max(0.0),
            ((centre + spread) / denom).min(1.0),
        )
    }

    /// `99.771% [99.767, 99.775] of 47,300,000`
    pub fn describe(&self) -> String {
        let (lo, hi) = self.interval();
        format!(
            "{:.3}% [{:.3}, {:.3}] of {}",
            self.rate() * 100.0,
            lo * 100.0,
            hi * 100.0,
            thousands(self.total)
        )
    }
}

/// Levenshtein distance, in characters rather than bytes.
///
/// Bengali characters are three bytes each in UTF-8, so a byte-wise distance
/// would count one wrong letter as three errors and quietly triple the error
/// rate. Two rows rather than a full matrix: the words are short but there are
/// half a million of them.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitution = prev[j] + usize::from(ca != cb);
            curr[j + 1] = substitution.min(prev[j + 1] + 1).min(curr[j] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// `1234567` as `1,234,567`.
pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_interval_never_escapes_zero_to_one() {
        // The case the normal approximation gets wrong: near-perfect accuracy.
        let (lo, hi) = Proportion::new(999_999, 1_000_000).interval();
        assert!(hi <= 1.0, "upper bound above 100%: {hi}");
        assert!(lo > 0.99);
        // And the other end.
        let (lo, hi) = Proportion::new(0, 1000).interval();
        assert!(lo >= 0.0, "lower bound below 0%: {lo}");
        assert!(hi < 0.01);
        // Perfect scores must not produce a nonsensical interval either.
        let (lo, hi) = Proportion::new(328, 328).interval();
        assert!((0.0..=1.0).contains(&lo) && hi <= 1.0);
    }

    #[test]
    fn a_wider_sample_gives_a_tighter_interval() {
        let narrow = Proportion::new(99, 100).interval();
        let wide = Proportion::new(99_000, 100_000).interval();
        assert!(
            (wide.1 - wide.0) < (narrow.1 - narrow.0),
            "more evidence must mean less uncertainty"
        );
    }

    #[test]
    fn distance_counts_characters_not_bytes() {
        // One wrong Bengali letter is one error, not the three bytes it takes.
        assert_eq!(edit_distance("বিবরণ", "বিবরন"), 1);
        assert_eq!(edit_distance("কর্মসূচি", "কর্মসূচি"), 0);
        assert_eq!(edit_distance("", "আমি"), 3);
        assert_eq!(edit_distance("আমি", ""), 3);
        assert_eq!(edit_distance("", ""), 0);
        // Insertion, deletion and substitution all cost one.
        assert_eq!(edit_distance("আম", "আমি"), 1);
        assert_eq!(edit_distance("আমি", "আম"), 1);
    }

    #[test]
    fn large_numbers_are_readable() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(6_459_067), "6,459,067");
    }
}
