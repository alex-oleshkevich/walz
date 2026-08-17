use secret_service::{EncryptionType, SecretService};
use std::collections::HashMap;

const APP_NAME: &str = "walz";

pub async fn store_secret(key: &str, value: &str, profile: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ss = SecretService::connect(EncryptionType::Dh).await?;
    let collection = ss.get_default_collection().await?;

    if collection.is_locked().await? {
        collection.unlock().await?;
    }

    let mut attributes = HashMap::new();
    attributes.insert("application", APP_NAME);
    attributes.insert("key", key);
    attributes.insert("profile", profile);

    let label = format!("{} - {} ({})", APP_NAME, key, profile);

    collection
        .create_item(
            &label,
            attributes,
            value.as_bytes(),
            true,
            "text/plain",
        )
        .await?;

    Ok(())
}

pub async fn get_secret(key: &str, profile: &str) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let ss = SecretService::connect(EncryptionType::Dh).await?;
    let collection = ss.get_default_collection().await?;

    if collection.is_locked().await? {
        collection.unlock().await?;
    }

    let mut attributes = HashMap::new();
    attributes.insert("application", APP_NAME);
    attributes.insert("key", key);
    attributes.insert("profile", profile);

    let items = collection.search_items(attributes).await?;

    if let Some(item) = items.first() {
        if item.is_locked().await? {
            item.unlock().await?;
        }
        let secret = item.get_secret().await?;
        Ok(Some(String::from_utf8(secret)?))
    } else {
        Ok(None)
    }
}

pub async fn delete_secret(key: &str, profile: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ss = SecretService::connect(EncryptionType::Dh).await?;
    let collection = ss.get_default_collection().await?;

    if collection.is_locked().await? {
        collection.unlock().await?;
    }

    let mut attributes = HashMap::new();
    attributes.insert("application", APP_NAME);
    attributes.insert("key", key);
    attributes.insert("profile", profile);

    let items = collection.search_items(attributes).await?;

    for item in items {
        item.delete().await?;
    }

    Ok(())
}

#[allow(dead_code)]
pub async fn list_secrets(profile: &str) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    let ss = SecretService::connect(EncryptionType::Dh).await?;
    let collection = ss.get_default_collection().await?;

    if collection.is_locked().await? {
        collection.unlock().await?;
    }

    let mut attributes = HashMap::new();
    attributes.insert("application", APP_NAME);
    attributes.insert("profile", profile);

    let items = collection.search_items(attributes).await?;

    let mut keys = Vec::new();
    for item in items {
        let attrs = item.get_attributes().await?;
        if let Some(key) = attrs.get("key") {
            keys.push(key.clone());
        }
    }

    Ok(keys)
}
