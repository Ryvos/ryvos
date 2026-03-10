use std::collections::HashMap;

use ryvos_core::config::ModelPricing;
use ryvos_core::types::BillingType;

/// Default pricing table: cents per million tokens (input, output).
fn default_pricing() -> Vec<(&'static str, u64, u64)> {
    vec![
        ("claude-sonnet-4", 300, 1500),
        ("claude-sonnet-4-20250514", 300, 1500),
        ("claude-opus-4", 1500, 7500),
        ("claude-opus-4-20250514", 1500, 7500),
        ("claude-haiku", 80, 400),
        ("claude-haiku-4-5-20251001", 80, 400),
        ("gpt-4o", 250, 1000),
        ("gpt-4o-mini", 15, 60),
        ("gpt-4-turbo", 1000, 3000),
        ("o1", 1500, 6000),
        ("o1-mini", 300, 1200),
    ]
}

/// Estimate cost in cents for a given usage.
///
/// Subscription billing always returns 0 (flat rate).
/// For API billing, looks up model pricing from overrides first,
/// then default table, then fallback (300/1500 = Sonnet-class pricing).
pub fn estimate_cost_cents(
    model: &str,
    _provider: &str,
    billing_type: BillingType,
    input_tokens: u64,
    output_tokens: u64,
    overrides: &HashMap<String, ModelPricing>,
) -> u64 {
    if billing_type == BillingType::Subscription {
        return 0;
    }

    let (input_rate, output_rate) = if let Some(pricing) = overrides.get(model) {
        (pricing.input_cents_per_mtok, pricing.output_cents_per_mtok)
    } else {
        match default_pricing().iter().find(|(m, _, _)| *m == model) {
            Some((_, i, o)) => (*i, *o),
            None => {
                // Check partial match (model ID contains a known name)
                let lower = model.to_lowercase();
                if lower.contains("opus") {
                    (1500, 7500)
                } else if lower.contains("haiku") {
                    (80, 400)
                } else if lower.contains("sonnet") {
                    (300, 1500)
                } else if lower.contains("gpt-4o-mini") {
                    (15, 60)
                } else if lower.contains("gpt-4o") {
                    (250, 1000)
                } else {
                    // Fallback: Sonnet-class pricing
                    (300, 1500)
                }
            }
        }
    };

    // cost = tokens * rate / 1_000_000
    let input_cost = input_tokens * input_rate / 1_000_000;
    let output_cost = output_tokens * output_rate / 1_000_000;
    input_cost + output_cost
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_is_free() {
        let cost = estimate_cost_cents(
            "claude-sonnet-4",
            "anthropic",
            BillingType::Subscription,
            1_000_000,
            1_000_000,
            &HashMap::new(),
        );
        assert_eq!(cost, 0);
    }

    #[test]
    fn api_sonnet_pricing() {
        // 1M input tokens at 300 cents/MTok = 300 cents
        // 1M output tokens at 1500 cents/MTok = 1500 cents
        let cost = estimate_cost_cents(
            "claude-sonnet-4",
            "anthropic",
            BillingType::Api,
            1_000_000,
            1_000_000,
            &HashMap::new(),
        );
        assert_eq!(cost, 1800);
    }

    #[test]
    fn override_pricing() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "custom-model".to_string(),
            ModelPricing {
                input_cents_per_mtok: 100,
                output_cents_per_mtok: 200,
            },
        );
        let cost = estimate_cost_cents(
            "custom-model",
            "custom",
            BillingType::Api,
            1_000_000,
            1_000_000,
            &overrides,
        );
        assert_eq!(cost, 300);
    }

    #[test]
    fn fallback_pricing() {
        // Unknown model gets Sonnet-class pricing
        let cost = estimate_cost_cents(
            "unknown-model-v9",
            "unknown",
            BillingType::Api,
            1_000_000,
            1_000_000,
            &HashMap::new(),
        );
        assert_eq!(cost, 1800);
    }
}
