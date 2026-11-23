//! Authenticate the complete checksum manifest before trusting any release file.
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;

pub fn verify(manifest: &str, signature: &str, version: &Version) -> Result<(), String> {
    #[cfg(not(test))]
    let key: [u8; 32] = STANDARD
        .decode(include_str!("../../assets/update-public-key.txt").trim())
        .map_err(|_| "invalid update public key")?
        .try_into()
        .map_err(|_| "invalid update public key length")?;
    #[cfg(test)]
    let key = ed25519_dalek::SigningKey::from_bytes(&[42; 32]).verifying_key().to_bytes();
    let key = VerifyingKey::from_bytes(&key).map_err(|_| "invalid update public key")?;
    let bytes = STANDARD.decode(signature.trim()).map_err(|_| "invalid update signature encoding")?;
    let sig = Signature::from_slice(&bytes).map_err(|_| "invalid update signature length")?;
    key.verify_strict(manifest.as_bytes(), &sig).map_err(|_| "update signature verification failed")?;
    // Bind the signed content to the API's version and this collaboration generation.
    let mut lines = manifest.lines();
    if lines.next() != Some(format!("# a11y-version={version}").as_str()) {
        return Err("signed update version does not match the release".into());
    }
    if lines.next() != Some(format!("# collaboration-protocol={}", a11y_net::PROTOCOL_VERSION).as_str()) {
        return Err("this update requires a coordinated collaboration server upgrade".into());
    }
    Ok(())
}

#[cfg(test)]
pub fn test_sign(manifest: &str) -> String {
    use ed25519_dalek::Signer;
    STANDARD.encode(ed25519_dalek::SigningKey::from_bytes(&[42; 32]).sign(manifest.as_bytes()).to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_manifest_is_bound_to_content_version_and_protocol() {
        let version = Version::parse("1.0.0").unwrap();
        let manifest = "# a11y-version=1.0.0\n# collaboration-protocol=3\nabc  app.zip\n";
        let sig = test_sign(manifest);
        assert!(verify(manifest, &sig, &version).is_ok());
        assert!(verify(&manifest.replace("abc", "bad"), &sig, &version).is_err());
        assert!(verify(manifest, &sig, &Version::parse("1.0.1").unwrap()).is_err());
        assert!(verify(manifest, "", &version).is_err());
        let incompatible = manifest.replace("protocol=3", "protocol=4");
        assert!(verify(&incompatible, &test_sign(&incompatible), &version).is_err());
        use ed25519_dalek::Signer;
        let wrong = STANDARD.encode(ed25519_dalek::SigningKey::from_bytes(&[43; 32]).sign(manifest.as_bytes()).to_bytes());
        assert!(verify(manifest, &wrong, &version).is_err());
    }
}
