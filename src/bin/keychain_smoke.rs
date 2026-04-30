//! Smoke test: store, read, and delete a Nextcloud password in the
//! user's running Secret Service keyring (gnome-keyring on this
//! laptop, or whatever's wired through D-Bus). Inlines the same
//! schema/attribute shape the production `keychain.rs` uses so this
//! exercises the same code path without a [lib] target restructure.
//!
//! Run with: `cargo run --bin keychain_smoke`
//! Won't survive without a keyring daemon — needs gnome-keyring-daemon
//! or equivalent reachable via D-Bus.

use std::collections::HashMap;

const SCHEMA: &str = "io.github.janekbt.Meditate.NextcloudSync";
const ATTR_URL: &str = "url";
const ATTR_USER: &str = "username";

const TEST_URL: &str = "https://smoke-test.invalid/nextcloud";
const TEST_USER: &str = "smoke-test-user";
const TEST_PASS_1: &str = "first-password-correct-horse-battery-staple";
const TEST_PASS_2: &str = "second-password-overwrites";

fn attrs<'a>(url: &'a str, username: &'a str) -> HashMap<&'a str, &'a str> {
    let mut a = HashMap::with_capacity(3);
    a.insert(oo7::XDG_SCHEMA_ATTRIBUTE, SCHEMA);
    a.insert(ATTR_URL, url);
    a.insert(ATTR_USER, username);
    a
}

fn main() {
    println!("=== keychain_smoke: oo7 against the running Secret Service ===\n");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(run());
    println!("\n=== keychain_smoke: ALL CHECKS PASSED ===");
}

async fn run() {
    // Step 1: open the keyring and unlock.
    let keyring = oo7::Keyring::new().await
        .expect("connect to keyring (need gnome-keyring-daemon running)");
    keyring.unlock().await.expect("unlock keyring");
    println!("✓ connected to keyring and unlocked");

    // Defensive cleanup: blow away anything from a previous failed run
    // so we start from a known-empty state. delete is a no-op if there's
    // no match.
    keyring.delete(&attrs(TEST_URL, TEST_USER)).await.expect("clear stale items");

    // Step 2: write a password.
    keyring.create_item(
        "Meditate sync — smoke test (delete me if you see this)",
        &attrs(TEST_URL, TEST_USER),
        TEST_PASS_1.as_bytes(),
        true,
    ).await.expect("create_item");
    println!("✓ wrote first password");

    // Step 3: read it back, verify equality.
    let items = keyring.search_items(&attrs(TEST_URL, TEST_USER)).await
        .expect("search_items");
    assert_eq!(items.len(), 1, "expected exactly one item, got {}", items.len());
    let secret = items[0].secret().await.expect("read secret");
    let pw = String::from_utf8(secret.to_vec()).expect("password is UTF-8");
    assert_eq!(pw, TEST_PASS_1);
    println!("✓ read back the same password ({} bytes)", pw.len());

    // Step 4: overwrite (replace=true) and verify the new one wins.
    keyring.create_item(
        "Meditate sync — smoke test (delete me if you see this)",
        &attrs(TEST_URL, TEST_USER),
        TEST_PASS_2.as_bytes(),
        true,
    ).await.expect("create_item replace");
    let items = keyring.search_items(&attrs(TEST_URL, TEST_USER)).await
        .expect("search_items after replace");
    assert_eq!(items.len(), 1,
        "replace=true must NOT create a duplicate; got {} items", items.len());
    let secret = items[0].secret().await.expect("read overwritten secret");
    let pw = String::from_utf8(secret.to_vec()).expect("password is UTF-8");
    assert_eq!(pw, TEST_PASS_2,
        "the second create_item should have overwritten the first");
    println!("✓ overwrite works (replace=true keeps a single item, new value)");

    // Step 5: search by URL+user combination — narrow query must NOT
    // accidentally find unrelated entries from other apps.
    let mut wrong_attrs = HashMap::new();
    wrong_attrs.insert(oo7::XDG_SCHEMA_ATTRIBUTE, SCHEMA);
    wrong_attrs.insert(ATTR_URL, "https://wrong-server.invalid");
    wrong_attrs.insert(ATTR_USER, TEST_USER);
    let no_match = keyring.search_items(&wrong_attrs).await
        .expect("search wrong attrs");
    assert!(no_match.is_empty(),
        "search with mismatching URL must return no matches, got {}", no_match.len());
    println!("✓ search by attributes is precise (different URL → no match)");

    // Step 6: delete and confirm gone.
    keyring.delete(&attrs(TEST_URL, TEST_USER)).await.expect("delete");
    let after_delete = keyring.search_items(&attrs(TEST_URL, TEST_USER)).await
        .expect("search after delete");
    assert!(after_delete.is_empty(),
        "post-delete search must return nothing, got {}", after_delete.len());
    println!("✓ delete removes the item; no orphan entries left in the keyring");
}
