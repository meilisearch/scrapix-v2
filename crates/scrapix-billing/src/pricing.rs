//! Volume-based tiered pricing.

/// Calculate the price in cents for a given number of credits.
/// Volume-based: the entire quantity is priced at the tier rate.
///
/// | Volume      | Per credit | Per 1K |
/// |-------------|-----------|--------|
/// | 1–999       | $0.010    | $10    |
/// | 1,000–4,999 | $0.008    | $8     |
/// | 5,000–9,999 | $0.007    | $7     |
/// | 10,000+     | $0.005    | $5     |
pub fn calculate_price_cents(credits: i64) -> i64 {
    // Unit price in tenths of a cent to avoid floating point
    let rate_tenths = if credits >= 10_000 {
        5 // $0.005 = 0.5 cents
    } else if credits >= 5_000 {
        7 // $0.007 = 0.7 cents
    } else if credits >= 1_000 {
        8 // $0.008 = 0.8 cents
    } else {
        10 // $0.010 = 1.0 cent
    };
    // cents = credits * rate_tenths / 10, ceiling
    (credits * rate_tenths + 9) / 10
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_price_small_volume() {
        // 100 credits at $0.01 each = $1.00 = 100 cents
        assert_eq!(calculate_price_cents(100), 100);
    }

    #[test]
    fn test_price_mid_volume() {
        // 1000 credits at $0.008 each = $8.00 = 800 cents
        assert_eq!(calculate_price_cents(1000), 800);
    }

    #[test]
    fn test_price_high_volume() {
        // 10000 credits at $0.005 each = $50.00 = 5000 cents
        assert_eq!(calculate_price_cents(10000), 5000);
    }
}
