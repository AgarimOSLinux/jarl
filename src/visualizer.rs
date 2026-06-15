use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::Source;
use rustfft::{num_complex::Complex, FftPlanner};

// ── Shared sample buffer ──────────────────────────────────────────────────────

pub type SampleBuffer = Arc<Mutex<VecDeque<f32>>>;

pub fn new_sample_buffer() -> SampleBuffer {
    Arc::new(Mutex::new(VecDeque::with_capacity(8192)))
}

// ── Capturing source ──────────────────────────────────────────────────────────
//
// Wraps a rodio Source and copies every sample into the shared buffer so
// the visualizer can read them without touching the audio pipeline.

pub struct CapturingSource<S: Source<Item = f32>> {
    inner:  S,
    buffer: SampleBuffer,
}

impl<S: Source<Item = f32>> CapturingSource<S> {
    pub fn new(inner: S, buffer: SampleBuffer) -> Self {
        Self { inner, buffer }
    }
}

impl<S: Source<Item = f32>> Iterator for CapturingSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = self.inner.next()?;
        if let Ok(mut buf) = self.buffer.try_lock() {
            buf.push_back(sample);
            while buf.len() > 8192 { buf.pop_front(); }
        }
        Some(sample)
    }
}

impl<S: Source<Item = f32>> Source for CapturingSource<S> {
    fn current_span_len(&self)  -> Option<usize>              { self.inner.current_span_len() }
    fn channels(&self)          -> std::num::NonZero<u16>     { self.inner.channels() }
    fn sample_rate(&self)       -> std::num::NonZero<u32>     { self.inner.sample_rate() }
    fn total_duration(&self)    -> Option<Duration>  { self.inner.total_duration() }
}

// ── FFT + band computation ────────────────────────────────────────────────────

const FFT_SIZE: usize = 2048;

/// Compute `n_bands` spectrum magnitudes from the shared buffer.
/// Returns values in [0.0, 1.0], logarithmically spaced from 20 Hz to 20 kHz.
pub fn compute_spectrum(buffer: &SampleBuffer, n_bands: usize) -> Vec<f32> {
    if n_bands == 0 { return vec![]; }

    let samples: Vec<f32> = {
        let Ok(buf) = buffer.try_lock() else { return vec![0.0; n_bands]; };
        if buf.len() < FFT_SIZE { return vec![0.0; n_bands]; }
        let start = buf.len() - FFT_SIZE;
        buf.iter().skip(start).copied().collect()
    };

    // Hann window
    let mut input: Vec<Complex<f32>> = samples.iter().enumerate().map(|(i, &s)| {
        let w = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32
                               / (FFT_SIZE - 1) as f32).cos());
        Complex::new(s * w, 0.0)
    }).collect();

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    fft.process(&mut input);

    let half = FFT_SIZE / 2;
    let magnitudes: Vec<f32> = input[..half].iter().map(|c| c.norm()).collect();

    // Logarithmic bands (20 Hz – 20 kHz, assuming 44100 Hz sample rate)
    let log_min = 20.0_f32.log10();
    let log_max = 20000.0_f32.log10();
    let freq_per_bin = 44100.0_f32 / FFT_SIZE as f32;

    let mut bands = vec![0.0_f32; n_bands];
    for (i, band) in bands.iter_mut().enumerate() {
        let t0 = i as f32 / n_bands as f32;
        let t1 = (i + 1) as f32 / n_bands as f32;
        let f_lo = 10.0_f32.powf(log_min + t0 * (log_max - log_min));
        let f_hi = 10.0_f32.powf(log_min + t1 * (log_max - log_min));
        let b_lo = (f_lo / freq_per_bin) as usize;
        let b_hi = ((f_hi / freq_per_bin) as usize + 1).min(half);
        if b_lo < b_hi {
            *band = magnitudes[b_lo..b_hi].iter().sum::<f32>() / (b_hi - b_lo) as f32;
        }
    }

    // Normalize
    let peak = bands.iter().cloned().fold(0.0_f32, f32::max);
    if peak > 0.0 { bands.iter_mut().for_each(|b| *b /= peak); }
    bands
}

// ── Smoothing ─────────────────────────────────────────────────────────────────

/// Exponential moving average smoothing: fast attack, slow decay.
pub fn smooth(current: &mut Vec<f32>, target: &[f32], attack: f32, decay: f32) {
    if current.len() != target.len() {
        *current = target.to_vec();
        return;
    }
    for (c, &t) in current.iter_mut().zip(target) {
        let alpha = if t > *c { attack } else { decay };
        *c = *c * (1.0 - alpha) + t * alpha;
    }
}
