#![allow(warnings)]

use core::convert::TryFrom;
use core::convert::TryInto;

use k256::ecdsa::{signature::Signer, signature::Verifier, Signature, SigningKey, VerifyingKey};
use p256::EncodedPoint;
use p384::NistP384;

use rand_chacha::rand_core::{RngCore, SeedableRng};
use rand_chacha::ChaChaRng;

use crate::ecdh::affine_math::ECSignerType;
use crate::ecdh::ecdh::{KeyExchange, ToBytes, ECDHNISTP384};
use elliptic_curve::sec1::EncodedPoint as EncodedPointP384;

use generic_array::GenericArray;

use crate::{CryptoError, Result};

// use libc_print::libc_println;

/// A trait to implement ECDSA signatures for any curve type.
///
/// As `RustCrypto` is yet to support (i.e. no `Projective arithmetic` yet) curves P384, p521 or Brainpool
/// I put together my own affine-point arithemtic impls leveraging types `SecretKey`, `PublicKey`, `EncodedPoint`
/// from the `elliptic-curve` crate.
///
/// For now - all methods in this trait return byte-arrays (this is just a stop-gap solution)
pub trait ECSignature {
    /// Type `r` represents the r component of an ECDSA signature.
    type r: AsRef<[u8]>;
    /// Type `s` represents the s component of an ECDSA signature.
    type s: AsRef<[u8]>;
    /// A type to hold the raw-signature i.e. `r + s in bytes`.
    type sbytes: AsRef<[u8]>;

    /// Generate a ECDSA keypair.
    ///
    /// - This function borrows `SigningKey` and `VerifyingKey` types
    /// from the p256 impl to compute ECDSASHA256 Signatures
    ///
    /// For other impls, we use a mix of `SecretKey`, `PublicKey`, `EncodedPoint` types.
    /// borrowed from the elliptic-curve crate.
    fn generate_keypair(&mut self, seed: [u8; 32]);
    /// Function to sign messages of arbitrary length.
    ///
    /// - Returns the `signature as byte-array` or an Error.
    ///
    /// Note - we use affine point arithmetic of ECDSA calculation for curves other than p256
    fn sign(&self, data: &[u8]) -> Result<Self::sbytes>;
    /// Function to verify a signature.
    ///
    /// - Returns a `bool` is successful or an Error.
    ///
    /// Note - we use affine point arithmetic of ECDSA calculation for curves other than p256
    fn verify(&self, data: &[u8], signature: &[u8]) -> Result<bool>;
    /// The raw `r` component of a signature in bytes
    fn r(s: Self::sbytes) -> Self::r;
    /// The raw `s` component of a signature in bytes
    fn s(s: Self::sbytes) -> Self::s;
}

/// A type to represent an ECDSA-SHA256 Signature. Tuple elements 0 and 1 represent the `signing and verifying` keys
pub struct ECDSASHA256Signature(pub [u8; 32], pub [u8; 64]);

impl ECSignature for ECDSASHA256Signature {
    type r = [u8; 32];
    type s = [u8; 32];
    type sbytes = [u8; 64];

    fn generate_keypair(&mut self, seed: [u8; 32]) {
        let mut rng = ChaChaRng::from_seed(seed); // test seed value.
        let mut dest = [0; 32];
        rng.fill_bytes(&mut dest);
        let signing_key = SigningKey::from_bytes(&dest).unwrap();
        let verifying_key = VerifyingKey::from(&signing_key);
        self.0 = signing_key.to_bytes().as_slice().try_into().unwrap();
        self.1 = verifying_key
            .to_encoded_point(false)
            .to_untagged_bytes()
            .unwrap()
            .as_slice()
            .try_into()
            .unwrap();
    }

    fn sign(&self, data: &[u8]) -> Result<Self::sbytes> {
        let signing_key = self.0;
        let signature = SigningKey::from_bytes(&signing_key)
            .map(|sk| sk.sign(data))
            .map_err(|_| CryptoError::SignatureError);
        signature
            .map(|s| s.as_ref().try_into().unwrap())
            .map_err(|_| CryptoError::SignatureError)
    }

    fn verify(&self, data: &[u8], signature: &[u8]) -> Result<bool> {
        let verifying_key = self.1;
        let generic_arr = GenericArray::clone_from_slice(&verifying_key);
        let encoded_vk = EncodedPoint::from_untagged_bytes(&generic_arr);
        let verifying_key = VerifyingKey::from_encoded_point(&encoded_vk)
            .map_err(|_| CryptoError::SignatureError)?;
        Ok(verifying_key
            .verify(
                data,
                &Signature::try_from(signature)
                    .expect("failed to parse serialized siganture bytes"),
            )
            .is_ok())
    }

    fn r(s: Self::sbytes) -> [u8; 32] {
        let r_bytes = s.as_ref()[..32].try_into().unwrap();
        r_bytes
    }

    fn s(s: Self::sbytes) -> [u8; 32] {
        let s_bytes = s.as_ref()[32..].try_into().unwrap();
        s_bytes
    }
}

/// A type to represent an ECDSA-SHA384 Signature. Tuple elements 0 and 1 represent the `signing and verifying` keys
pub struct ECDSASHA384Signature(pub [u8; 48], pub EncodedPointP384<NistP384>);

impl ECSignature for ECDSASHA384Signature {
    type r = [u8; 48];
    type s = [u8; 48];
    type sbytes = [u8; 96]; // signature bytes

    fn generate_keypair(&mut self, seed: [u8; 32]) {
        let signing_key = ECDHNISTP384::<48>::generate_private_key(seed); // reusing functionality from ECDH module
        let verifying_key = ECDHNISTP384::<48>::generate_public_key(&signing_key);
        self.0 = signing_key.to_bytes().as_slice().try_into().unwrap();
        self.1 = verifying_key.0;
    }

    fn sign(&self, data: &[u8]) -> Result<Self::sbytes> {
        let (r, s) = ECSignerType::<48>::sign(data, &self.0);
        let r_bytes: [u8; 48] = r.to_bytes_be().1.as_slice().try_into().unwrap();
        let s_bytes: [u8; 48] = s.to_bytes_be().1.as_slice().try_into().unwrap();
        let mut sbytes = [0; 96];
        let _temp: () = r_bytes
            .iter()
            .chain(s_bytes.iter())
            .enumerate()
            .map(|(i, x)| sbytes[i] = *x)
            .collect();
        Ok(sbytes)
    }

    fn verify(&self, data: &[u8], signature: &[u8]) -> Result<bool> {
        let verification_status = ECSignerType::<48>::verify(data, signature, self.1);
        verification_status
    }

    fn r(s: Self::sbytes) -> [u8; 48] {
        let r_bytes = s.as_ref()[..48].try_into().unwrap();
        r_bytes
    }

    fn s(s: Self::sbytes) -> [u8; 48] {
        let s_bytes = s.as_ref()[48..].try_into().unwrap();
        s_bytes
    }
}
