use data_encoding::HEXLOWER;
use ring::hmac;

const SIGNATURE_HEX_LEN: usize = 32;

/// Sign a URL using HMAC-SHA256, truncated to 128 bits.
pub fn sign(url: &str, key: &str) -> String {
   let key = hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes());
   let tag = hmac::sign(&key, url.as_bytes());
   let full = HEXLOWER.encode(tag.as_ref());
   full[..SIGNATURE_HEX_LEN].to_owned()
}

/// Verify an HMAC signature.
pub fn verify(url: &str, signature: &str, key: &str) -> bool {
   let expected = sign(url, key);
   constant_time_compare(&expected, signature)
}

/// Constant-time string comparison to prevent timing attacks.
fn constant_time_compare(left: &str, right: &str) -> bool {
   if left.len() != right.len() {
      return false;
   }

   let mut result = 0_u8;
   for (lhs, rhs) in left.bytes().zip(right.bytes()) {
      result |= lhs ^ rhs;
   }

   result == 0
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_hmac_sign() {
      let url = "https://video.twimg.com/ext_tw_video/123/pu/vid/1280x720/test.mp4";
      let key = "secretkey";

      let sig1 = sign(url, key);
      let sig2 = sign(url, key);

      assert_eq!(sig1, sig2);
      assert!(verify(url, &sig1, key));
      assert!(!verify(url, "invalid", key));
   }
}
