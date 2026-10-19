// What the speakers are playing, as 128 levels for wallpapers that ask for sound ("--audio true", like Lively).
//
// A thread records the default speakers in loopback (a copy of what's playing; nothing is recorded to disk) and
// keeps the latest 2048 samples. The levels are a spectrum on a pitch-like scale from 20 Hz (index 0) up to about
// 12 kHz, each from 0 to 1. It only runs while a wallpaper that wants sound is playing.

use std::{
    f32::consts::PI,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use windows::Win32::{
    Media::{
        Audio::{
            eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
            AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX,
            WAVEFORMATEXTENSIBLE,
        },
        KernelStreaming::{KSDATAFORMAT_SUBTYPE_PCM, WAVE_FORMAT_EXTENSIBLE},
        Multimedia::{KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, WAVE_FORMAT_IEEE_FLOAT},
    },
    System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED},
};

use crate::log;

pub const BANDS: usize = 128;
const N: usize = 2048;

pub struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    levels: Arc<Mutex<(Vec<f32>, Instant)>>,
}

impl Capture {
    pub fn start() -> Capture {
        let stop = Arc::new(AtomicBool::new(false));
        let levels = Arc::new(Mutex::new((vec![0.0; BANDS], Instant::now())));
        let (s, l) = (stop.clone(), levels.clone());
        let thread = std::thread::Builder::new().name("audio".into()).spawn(move || run(s, l)).ok();
        Capture { stop, thread, levels }
    }

    /// The latest levels as a JavaScript array, or None if nothing new has been heard for a while.
    pub fn levels_js(&self) -> Option<String> {
        let g = self.levels.lock().ok()?;
        if g.1.elapsed() > Duration::from_millis(400) {
            return None;
        }
        let mut s = String::with_capacity(BANDS * 6);
        s.push('[');
        for (i, v) in g.0.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            if *v <= 0.0005 {
                s.push('0');
            } else {
                s.push_str(&format!("{:.3}", v));
            }
        }
        s.push(']');
        Some(s)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

enum Sample {
    F32,
    I16,
    I32,
}

fn run(stop: Arc<AtomicBool>, out: Arc<Mutex<(Vec<f32>, Instant)>>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut analyser = Analyser::new();
    let mut last_error = String::new();
    while !stop.load(Ordering::Acquire) {
        if let Err(e) = unsafe { capture(&stop, &out, &mut analyser) } {
            let msg = e.to_string();
            if msg != last_error {
                log!("listening to the speakers stopped: {msg}");
                last_error = msg;
            }
        }
        // The device changed or went away: try again shortly.
        for _ in 0..10 {
            if stop.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    unsafe { CoUninitialize() };
}

unsafe fn capture(
    stop: &AtomicBool,
    out: &Mutex<(Vec<f32>, Instant)>,
    an: &mut Analyser,
) -> windows::core::Result<()> {
    let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
    let dev_id = dev.GetId()?.to_string().unwrap_or_default();
    let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
    let fmt_ptr = client.GetMixFormat()?;
    let fmt: WAVEFORMATEX = *fmt_ptr;
    let kind = {
        let tag = fmt.wFormatTag as u32;
        let sub = if tag == WAVE_FORMAT_EXTENSIBLE {
            Some((*(fmt_ptr as *const WAVEFORMATEXTENSIBLE)).SubFormat)
        } else {
            None
        };
        let float = tag == WAVE_FORMAT_IEEE_FLOAT || sub == Some(KSDATAFORMAT_SUBTYPE_IEEE_FLOAT);
        let pcm = tag == 1 || sub == Some(KSDATAFORMAT_SUBTYPE_PCM);
        match (float, pcm, fmt.wBitsPerSample) {
            (true, _, 32) => Some(Sample::F32),
            (_, true, 16) => Some(Sample::I16),
            (_, true, 32) => Some(Sample::I32),
            _ => None,
        }
    };
    let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 200_000, 0, fmt_ptr, None);
    CoTaskMemFree(Some(fmt_ptr as *const _));
    init?;
    let Some(kind) = kind else {
        return Err(windows::core::Error::new(windows::Win32::Foundation::E_NOTIMPL, "the speakers use a sample format that isn't supported"));
    };
    let cap: IAudioCaptureClient = client.GetService()?;
    let channels = fmt.nChannels.max(1) as usize;
    an.rate = fmt.nSamplesPerSec as f32;
    client.Start()?;
    let mut last_analysis = Instant::now();
    let mut last_data = Instant::now();
    let mut last_device_check = Instant::now();
    let result = (|| -> windows::core::Result<()> {
        while !stop.load(Ordering::Acquire) {
            loop {
                let n = cap.GetNextPacketSize()?;
                if n == 0 {
                    break;
                }
                let mut data = std::ptr::null_mut();
                let mut frames = 0u32;
                let mut flags = 0u32;
                cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                for f in 0..frames as usize {
                    let mut sum = 0.0f32;
                    if !silent && !data.is_null() {
                        for c in 0..channels {
                            let i = f * channels + c;
                            sum += match kind {
                                Sample::F32 => *(data as *const f32).add(i),
                                Sample::I16 => *(data as *const i16).add(i) as f32 / 32768.0,
                                Sample::I32 => *(data as *const i32).add(i) as f32 / 2147483648.0,
                            };
                        }
                    }
                    an.push(sum / channels as f32);
                }
                cap.ReleaseBuffer(frames)?;
                last_data = Instant::now();
            }
            if last_analysis.elapsed() >= Duration::from_millis(30) {
                last_analysis = Instant::now();
                // Nothing playing: Windows sends no packets at all, so fade out by feeding silence.
                if last_data.elapsed() > Duration::from_millis(60) {
                    for _ in 0..(an.rate as usize * 30 / 1000) {
                        an.push(0.0);
                    }
                }
                let levels = an.analyse();
                if let Ok(mut g) = out.lock() {
                    g.0.copy_from_slice(&levels);
                    g.1 = Instant::now();
                }
            }
            // Follow the default speakers when they change (headphones plugged in, say).
            if last_device_check.elapsed() > Duration::from_secs(3) {
                last_device_check = Instant::now();
                let now_id = en.GetDefaultAudioEndpoint(eRender, eConsole).and_then(|d| d.GetId()).map(|p| p.to_string().unwrap_or_default());
                if now_id.map(|i| i != dev_id).unwrap_or(true) {
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    })();
    let _ = client.Stop();
    result
}

struct Analyser {
    ring: Vec<f32>,
    pos: usize,
    window: Vec<f32>,
    rate: f32,
    smooth: Vec<f32>,
}

impl Analyser {
    fn new() -> Analyser {
        let window = (0..N).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / (N as f32 - 1.0)).cos()).collect();
        Analyser { ring: vec![0.0; N], pos: 0, window, rate: 48000.0, smooth: vec![0.0; BANDS] }
    }

    fn push(&mut self, s: f32) {
        self.ring[self.pos] = s;
        self.pos = (self.pos + 1) % N;
    }

    fn analyse(&mut self) -> Vec<f32> {
        let mut re = vec![0.0f32; N];
        let mut im = vec![0.0f32; N];
        for i in 0..N {
            re[i] = self.ring[(self.pos + i) % N] * self.window[i];
        }
        fft(&mut re, &mut im);
        // Amplitude per bin, scaled so a full-scale sine reads 1.
        let wsum: f32 = self.window.iter().sum();
        let half = N / 2;
        let mag: Vec<f32> = (0..half).map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt() * 2.0 / wsum).collect();
        let hz_per_bin = self.rate / N as f32;
        let mel = |f: f32| 2595.0 * (1.0 + f / 700.0).log10();
        let inv = |m: f32| 700.0 * (10f32.powf(m / 2595.0) - 1.0);
        let (m0, m1) = (mel(20.0), mel((self.rate / 2.0).min(12000.0)));
        let mut out = vec![0.0f32; BANDS];
        for b in 0..BANDS {
            let f_lo = inv(m0 + (m1 - m0) * b as f32 / BANDS as f32);
            let f_hi = inv(m0 + (m1 - m0) * (b + 1) as f32 / BANDS as f32);
            let k_lo = ((f_lo / hz_per_bin).floor() as usize).min(half - 1);
            let k_hi = ((f_hi / hz_per_bin).ceil() as usize).clamp(k_lo + 1, half);
            let peak = mag[k_lo..k_hi].iter().cloned().fold(0.0f32, f32::max);
            // Decibels mapped to 0..1: -70 dB is silence, -10 dB is as loud as it gets.
            let db = 20.0 * peak.max(1e-7).log10();
            let v = ((db + 70.0) / 60.0).clamp(0.0, 1.0);
            // Rise at once, fall gently, so bars don't flicker.
            let s = &mut self.smooth[b];
            *s = if v > *s { v } else { *s * 0.7 + v * 0.3 };
            out[b] = *s;
        }
        out
    }
}

/// In-place radix-2 FFT.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(an: &mut Analyser, hz: f32, amp: f32) {
        for i in 0..N {
            an.push(amp * (2.0 * PI * hz * i as f32 / an.rate).sin());
        }
    }

    fn band_of(rate: f32, hz: f32) -> usize {
        let mel = |f: f32| 2595.0 * (1.0 + f / 700.0).log10();
        let (m0, m1) = (mel(20.0), mel((rate / 2.0).min(12000.0)));
        (((mel(hz) - m0) / (m1 - m0)) * BANDS as f32) as usize
    }

    #[test]
    fn tone_lands_in_its_band() {
        for hz in [60.0f32, 440.0, 3000.0] {
            let mut an = Analyser::new();
            feed(&mut an, hz, 0.5);
            let levels = an.analyse();
            let b = band_of(an.rate, hz);
            let peak = (0..BANDS).max_by(|&a, &c| levels[a].partial_cmp(&levels[c]).unwrap()).unwrap();
            assert!((peak as i32 - b as i32).abs() <= 2, "{hz} Hz peaked in band {peak}, expected about {b}");
            assert!(levels[peak] > 0.85, "{hz} Hz at half volume should read loud, got {}", levels[peak]);
            let far = if b > 64 { 5 } else { 120 };
            assert!(levels[far] < 0.35, "{hz} Hz leaked into band {far}: {}", levels[far]);
        }
    }

    #[test]
    fn silence_reads_zero() {
        let mut an = Analyser::new();
        for _ in 0..N {
            an.push(0.0);
        }
        assert!(an.analyse().iter().all(|v| *v == 0.0));
    }

    #[test]
    fn bass_is_in_the_first_bands() {
        // Clawd Tunes counts the first 6 bands as the bass, so a kick drum (around 50-100 Hz) must land there.
        assert!(band_of(48000.0, 100.0) < 6);
        assert!(band_of(44100.0, 100.0) < 6);
    }
}
