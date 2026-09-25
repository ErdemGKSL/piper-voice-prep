use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use nnnoiseless::{DenoiseState, FRAME_SIZE};
use rubato::{FftFixedInOut, Resampler};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as AudioError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use walkdir::WalkDir;
mod tui;

const RATE: u32 = 22_050;

#[derive(Parser)]
#[command(version, about = "Prepare speaker folders as Piper training datasets")]
struct Cli {
    /// Root folder for interactive mode (defaults to executable's folder).
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Action>,
}

#[derive(Subcommand)]
enum Action {
    /// Clean and split audio from every immediate speaker folder.
    Prepare {
        /// Directory containing one folder per speaker.
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Smallest clip in seconds.
        #[arg(long, default_value_t = 1.5)]
        min_seconds: f32,
        /// Largest clip in seconds.
        #[arg(long, default_value_t = 12.0)]
        max_seconds: f32,
        /// Optional whisper.cpp model; runs whisper-cli for each clip.
        #[arg(long)]
        whisper_model: Option<PathBuf>,
        /// Path to whisper-cli when using --whisper-model.
        #[arg(long, default_value = "whisper-cli")]
        whisper_bin: PathBuf,
        /// Language hint for whisper.cpp, e.g. tr.
        #[arg(long, default_value = "tr")]
        language: String,
    },
    /// Rebuild Piper metadata after editing transcripts.tsv.
    Finalize {
        #[arg(default_value = ".")]
        root: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Action::Prepare { root, min_seconds, max_seconds, whisper_model, whisper_bin, language }) => {
            if min_seconds < 0.2 || max_seconds <= min_seconds { bail!("invalid min/max duration"); }
            let root = root.canonicalize().context("root directory does not exist")?;
            if let Some(ref model) = whisper_model { if !model.is_file() { bail!("Whisper model missing: {}", model.display()); } }
            for speaker in speaker_dirs(&root)? {
                if let Err(e) = prepare_speaker(&speaker, min_seconds, max_seconds, whisper_model.as_deref(), &whisper_bin, &language) {
                    eprintln!("ERROR {}: {e:#}", speaker.display());
                }
            }
        }
        Some(Action::Finalize { root }) => {
            for speaker in speaker_dirs(&root)? {
                let output = speaker.join("output");
                if output.join("transcripts.tsv").exists() { finalize(&output)?; }
            }
        }
        None => tui::run(cli.root.unwrap_or_else(default_root))?,
    }
    Ok(())
}

fn default_root() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn speaker_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs = fs::read_dir(root)?.filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir() && !t.is_symlink()).unwrap_or(false))
        .map(|e| e.path()).collect::<Vec<_>>();
    dirs.sort();
    Ok(dirs)
}

fn audio_files(speaker: &Path) -> Vec<PathBuf> {
    let mut files = WalkDir::new(speaker).follow_links(false).into_iter()
        .filter_entry(|e| e.file_name() != "output")
        .filter_map(|e| e.ok()).filter(|e| e.file_type().is_file())
        .map(|e| e.into_path()).filter(|p| {
            p.extension().and_then(|s| s.to_str()).map(|s|
                matches!(s.to_ascii_lowercase().as_str(), "wav"|"m4a"|"mp3"|"flac"|"ogg"|"opus"|"aac"|"aiff"|"aif"))
                .unwrap_or(false)
        }).collect::<Vec<_>>();
    files.sort();
    files
}

fn prepare_speaker(speaker: &Path, min_sec: f32, max_sec: f32, model: Option<&Path>, whisper_bin: &Path, language: &str) -> Result<()> {
    let files = audio_files(speaker);
    if files.is_empty() { return Ok(()); }
    let output = speaker.join("output");
    let wav_dir = output.join("wav");
    fs::create_dir_all(&wav_dir)?;
    let mut transcripts = read_transcripts(&output.join("transcripts.tsv"))?;
    let mut generated = 0usize;
    for (file_index, file) in files.iter().enumerate() {
        eprintln!("{}: {}", speaker.file_name().unwrap().to_string_lossy(), file.display());
        let (input, input_rate) = match decode(file) {
            Ok(v) => v,
            Err(e) => { eprintln!("  skipped: {e:#}"); continue; }
        };
        let audio48 = resample(&input, input_rate, 48_000)?;
        drop(input);
        let clean = denoise(&audio48);
        drop(audio48);
        let audio = resample(&clean, 48_000, RATE)?;
        drop(clean);
        let spans = segment(&audio, min_sec, max_sec);
        let mut written = 0;
        for (clip_index, (start, end)) in spans.into_iter().enumerate() {
            let name = format!("src{:03}_clip{:05}.wav", file_index + 1, clip_index + 1);
            let destination = wav_dir.join(&name);
            write_wav(&destination, &audio[start..end])?;
            if !transcripts.contains_key(&name) {
                let text = if let Some(model) = model {
                    match transcribe(whisper_bin, model, language, &destination) {
                        Ok(t) => t,
                        Err(e) => { eprintln!("  ASR failed on {name}: {e:#}"); String::new() }
                    }
                } else { String::new() };
                transcripts.insert(name, text);
            }
            written += 1;
        }
        eprintln!("  {written} clips");
        generated += written;
    }
    write_transcripts(&output.join("transcripts.tsv"), &transcripts)?;
    finalize(&output)?;
    eprintln!("{}: {generated} clips prepared", speaker.display());
    Ok(())
}

fn decode(path: &Path) -> Result<(Vec<f32>, u32)> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|x| x.to_str()) { hint.with_extension(ext); }
    let probed = symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())?;
    let mut format = probed.format;
    let track = format.default_track().context("no audio track")?;
    let id = track.id;
    let rate = track.codec_params.sample_rate.context("unknown sample rate")?;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let mut out = Vec::<f32>::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(AudioError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != id { continue; }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(AudioError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count();
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buffer.copy_interleaved_ref(decoded);
        for frame in buffer.samples().chunks_exact(channels) {
            out.push(frame.iter().sum::<f32>() / channels as f32);
        }
    }
    if out.is_empty() { bail!("empty audio"); }
    Ok((out, rate))
}

fn resample(input: &[f32], from: u32, to: u32) -> Result<Vec<f32>> {
    if from == to { return Ok(input.to_vec()); }
    let mut converter = FftFixedInOut::<f32>::new(from as usize, to as usize, 4800, 1)?;
    let chunk = converter.input_frames_next();
    let expected = ((input.len() as f64 * to as f64 / from as f64).round()) as usize;
    let mut out = Vec::with_capacity(expected + chunk);
    for part in input.chunks(chunk) {
        let result = if part.len() == chunk { converter.process(&[part], None)? }
                     else { converter.process_partial(Some(&[part]), None)? };
        out.extend_from_slice(&result[0]);
    }
    out.truncate(expected);
    Ok(out)
}

fn denoise(input: &[f32]) -> Vec<f32> {
    let mut state = DenoiseState::new();
    let mut output = Vec::with_capacity(input.len());
    let mut frame_in = [0.0_f32; FRAME_SIZE];
    let mut frame_out = [0.0_f32; FRAME_SIZE];
    for chunk in input.chunks(FRAME_SIZE) {
        frame_in.fill(0.0);
        for (dst, src) in frame_in.iter_mut().zip(chunk) { *dst = (*src * 32768.0).clamp(-32768.0, 32767.0); }
        state.process_frame(&mut frame_out, &frame_in);
        output.extend(frame_out[..chunk.len()].iter().map(|v| (v / 32768.0).clamp(-1.0, 1.0)));
    }
    output
}

fn segment(audio: &[f32], min_sec: f32, max_sec: f32) -> Vec<(usize, usize)> {
    let frame = (RATE as f32 * 0.02) as usize;
    let energy: Vec<f32> = audio.chunks(frame).map(|s| (s.iter().map(|v| v*v).sum::<f32>() / s.len() as f32).sqrt()).collect();
    if energy.is_empty() { return Vec::new(); }
    let mut sorted = energy.clone();
    sorted.sort_by(|a,b| a.total_cmp(b));
    let noise = sorted[sorted.len()/5];
    let threshold = (noise * 2.5).max(0.006).min(0.04);
    let speech: Vec<bool> = energy.iter().map(|&e| e > threshold).collect();
    let mut spans = Vec::new();
    let mut begin = None;
    let mut last_voice = 0;
    for (i, &active) in speech.iter().enumerate() {
        if active { if begin.is_none() { begin = Some(i); } last_voice = i; }
        if let Some(start) = begin {
            if i.saturating_sub(last_voice) > 25 {
                spans.push((start.saturating_sub(10), (last_voice + 11).min(speech.len())));
                begin = None;
            }
        }
    }
    if let Some(start) = begin { spans.push((start.saturating_sub(10), (last_voice+11).min(speech.len()))); }
    let mut result = Vec::new();
    let min_frames = (min_sec / 0.02) as usize;
    let max_frames = (max_sec / 0.02) as usize;
    for (start, end) in spans {
        let mut pos = start;
        while end.saturating_sub(pos) > max_frames {
            let target = pos + max_frames;
            let search_start = pos + max_frames * 2 / 3;
            let cut = (search_start..target).min_by(|a,b| energy[*a].total_cmp(&energy[*b])).unwrap_or(target);
            if cut.saturating_sub(pos) >= min_frames { result.push((pos*frame, (cut*frame).min(audio.len()))); }
            pos = cut;
        }
        if end.saturating_sub(pos) >= min_frames { result.push((pos*frame, (end*frame).min(audio.len()))); }
    }
    result
}

fn write_wav(path: &Path, audio: &[f32]) -> Result<()> {
    let mut writer = hound::WavWriter::create(path, hound::WavSpec { channels: 1, sample_rate: RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int })?;
    let peak = audio.iter().fold(0.0_f32, |a, v| a.max(v.abs()));
    let gain = if peak > 0.0 { (0.92 / peak).min(3.0) } else { 1.0 };
    for &v in audio { writer.write_sample((v * gain * 32767.0).clamp(-32768.0, 32767.0) as i16)?; }
    writer.finalize()?;
    Ok(())
}

fn transcribe(bin: &Path, model: &Path, language: &str, wav: &Path) -> Result<String> {
    let base = wav.with_extension("");
    let status = Command::new(bin).arg("-m").arg(model).arg("-f").arg(wav)
        .arg("-l").arg(language).arg("-otxt").arg("-of").arg(&base).arg("-nt").status()?;
    if !status.success() { bail!("whisper-cli exited with {status}"); }
    let txt = fs::read_to_string(base.with_extension("txt"))?;
    let _ = fs::remove_file(base.with_extension("txt"));
    Ok(clean_text(&txt))
}

fn clean_text(s: &str) -> String { s.replace(['\n','\r','\t','|'], " ").split_whitespace().collect::<Vec<_>>().join(" ") }

fn read_transcripts(path: &Path) -> Result<BTreeMap<String, String>> {
    let mut rows = BTreeMap::new();
    if !path.exists() { return Ok(rows); }
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        if let Some((name, text)) = line.split_once('\t') { rows.insert(name.to_owned(), text.to_owned()); }
    }
    Ok(rows)
}

fn write_transcripts(path: &Path, rows: &BTreeMap<String, String>) -> Result<()> {
    let mut file = File::create(path)?;
    for (name, text) in rows { writeln!(file, "{name}\t{}", clean_text(text))?; }
    Ok(())
}

fn finalize(output: &Path) -> Result<()> {
    let rows = read_transcripts(&output.join("transcripts.tsv"))?;
    let reviews = read_reviews(&output.join("reviews.tsv"))?;
    let mut modern = File::create(output.join("metadata.csv"))?;
    let mut legacy = File::create(output.join("metadata_legacy.csv"))?;
    let mut count = 0;
    for (name, text) in &rows {
        let text = clean_text(text);
        if text.is_empty() || reviews.get(name).map(String::as_str) != Some("approved") || !output.join("wav").join(name).is_file() { continue; }
        writeln!(modern, "{name}|{text}")?;
        writeln!(legacy, "{}|{text}", name.trim_end_matches(".wav"))?;
        count += 1;
    }
    let _ = count;
    Ok(())
}

fn read_reviews(path: &Path) -> Result<BTreeMap<String, String>> {
    let mut rows = BTreeMap::new();
    if !path.exists() { return Ok(rows); }
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        if let Some((name, status)) = line.split_once('\t') { rows.insert(name.to_owned(), status.to_owned()); }
    }
    Ok(rows)
}

fn write_reviews(path: &Path, rows: &BTreeMap<String, String>) -> Result<()> {
    let mut file = File::create(path)?;
    for (name, status) in rows { writeln!(file, "{name}\t{status}")?; }
    Ok(())
}
