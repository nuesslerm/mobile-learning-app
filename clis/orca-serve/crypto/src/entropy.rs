/// The only source of randomness the core uses, so tests can pin every nonce and key.
pub trait Entropy {
    fn fill(&self, buf: &mut [u8]);
}

pub struct SystemEntropy;

impl Entropy for SystemEntropy {
    fn fill(&self, buf: &mut [u8]) {
        getrandom::getrandom(buf).expect("the OS random source is unavailable");
    }
}

pub fn random_array<const N: usize>(entropy: &dyn Entropy) -> [u8; N] {
    let mut out = [0u8; N];
    entropy.fill(&mut out);
    out
}
