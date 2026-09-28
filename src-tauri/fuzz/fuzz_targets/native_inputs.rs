#![no_main]

use deltamod_product_contracts::ValidatedRelativePath;
use deltamod_protocol_domain::{parse_asset_request, parse_deep_link, parse_range, plan_range};
use deltamod_updater_launch_runtime::steam_discovery::steam_library_roots;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };

    let _ = steam_library_roots(text);
    let _ = parse_deep_link(text);
    let _ = parse_range(text);
    let _ = plan_range(Some(text), (data.len() as u64).saturating_add(1));
    let _ = parse_asset_request(text);
    let _ = ValidatedRelativePath::parse(text);
});
