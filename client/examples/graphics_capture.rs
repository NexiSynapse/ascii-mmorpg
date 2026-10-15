//! Headless, fixed-input capture of the real GPU scene and ASCII compute pipeline.
//!
//! Run with an explicit output directory (existing capture files are never overwritten):
//! `cargo run -p ascii-mmorpg-client --example graphics_capture -- <output-dir> [width] [height] [frame]`
//! Defaults: 160 x 80 cells, animation frame 24, sample_world(), rain, idle player.
//! JSON cells are row-major; glyphs are Unicode code points, RGB channels are raw u32s,
//! and depth_bits preserves the exact GPU depth representation alongside numeric depth.
//! Captures can vary across adapters/drivers; compare using the recorded adapter metadata.

#[cfg(not(feature = "gpu"))]
fn main() -> std::process::ExitCode {
    eprintln!("graphics_capture requires the gpu feature; rebuild with --features gpu");
    std::process::ExitCode::FAILURE
}

#[cfg(feature = "gpu")]
fn main() -> std::process::ExitCode {
    match capture::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("graphics_capture: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "gpu")]
mod capture {
    use std::error::Error;
    use std::ffi::OsString;
    use std::fs::{self, OpenOptions};
    use std::io::{self, BufWriter, Write};
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    use ascii_mmorpg_client::gpu::context::GpuContext;
    use ascii_mmorpg_client::gpu::mesh::MeshBuffer;
    use ascii_mmorpg_client::gpu::pipeline::SSAA_FACTOR;
    use ascii_mmorpg_client::gpu::{GpuCell, GpuPipeline, GpuRenderer, SceneBatches};
    use ascii_mmorpg_client::{renderer::WeatherMode, sample_world};

    const WARMUP_FRAMES: usize = 5;
    const MEASURED_FRAMES: usize = 30;
    const OUTPUT_FILES: [&str; 4] = ["scene.json", "scene.ansi", "scene.txt", "benchmark.txt"];

    pub fn run() -> Result<(), Box<dyn Error>> {
        let mut args = std::env::args_os().skip(1);
        let output = args
            .next()
            .filter(|arg| !arg.is_empty())
            .ok_or("usage: graphics_capture <output-dir> [width=160] [height=80] [frame=24]")?;
        let width = parse_number(args.next(), 160_i16, "width")?;
        let height = parse_number(args.next(), 80_i16, "height")?;
        let frame = parse_number(args.next(), 24_u64, "frame")?;
        if args.next().is_some() || width < 20 || height < 10 {
            return Err("expected width >= 20, height >= 10, and at most four arguments".into());
        }

        // Verify the explicit destination before creating directories or initializing the GPU.
        let output = prepare_output_dir(Path::new(&output))?;
        let ctx = GpuContext::init()?;
        let limits = ctx.device.limits();
        let cell_bytes = width as u64 * height as u64 * std::mem::size_of::<GpuCell>() as u64;
        if width as u32 * SSAA_FACTOR > limits.max_texture_dimension_2d
            || height as u32 * SSAA_FACTOR > limits.max_texture_dimension_2d
            || cell_bytes > limits.max_storage_buffer_binding_size as u64
            || cell_bytes > limits.max_buffer_size
        {
            return Err("capture dimensions exceed this GPU device's texture/buffer limits".into());
        }
        let adapter = ctx.adapter.get_info();
        let mut pipeline = GpuPipeline::new(&ctx, width as u32, height as u32)?;
        let mut mesh = MeshBuffer::new();
        let mut batches = SceneBatches::new();
        let world = sample_world();
        let mut render = || {
            mesh.clear();
            batches.clear();
            // Explicit idle phase avoids the realtime renderer's wall-clock walk tracking.
            let (camera, lighting) = GpuRenderer::populate_scene(
                width,
                height,
                WeatherMode::Rain,
                &world,
                frame,
                0.0,
                &mut mesh,
                &mut batches,
            );
            pipeline.render_and_readback_batched(&ctx, &mesh, &batches, &camera, &lighting)
        };

        let mut cells = Vec::new();
        for _ in 0..WARMUP_FRAMES {
            cells = render()?;
        }
        validate_cells(&cells, width as usize * height as usize)?;

        let mut timings_ms = Vec::with_capacity(MEASURED_FRAMES);
        for _ in 0..MEASURED_FRAMES {
            let start = Instant::now();
            let repeated = render()?;
            timings_ms.push(start.elapsed().as_secs_f64() * 1000.0);
            // Validation is outside the measured scene-build-through-readback interval.
            if bytemuck::cast_slice::<GpuCell, u8>(&repeated)
                != bytemuck::cast_slice::<GpuCell, u8>(&cells)
            {
                return Err(
                    "identical scene inputs produced different GPU cells during capture".into(),
                );
            }
        }

        write_json(
            &output.join(OUTPUT_FILES[0]),
            width,
            height,
            frame,
            &adapter,
            &cells,
        )?;
        write_rows(&output.join(OUTPUT_FILES[1]), &cells, width as usize, true)?;
        write_rows(&output.join(OUTPUT_FILES[2]), &cells, width as usize, false)?;

        let mut sorted = timings_ms.clone();
        sorted.sort_by(f64::total_cmp);
        let median = (sorted[MEASURED_FRAMES / 2 - 1] + sorted[MEASURED_FRAMES / 2]) / 2.0;
        // Nearest-rank 95th percentile: ceil(0.95 * 30) = sample 29.
        // Spell out the integer ceiling to keep the example compatible with the
        // workspace MSRV (Rust 1.70), before `usize::div_ceil` was stabilized.
        let p95 = sorted[((95 * MEASURED_FRAMES + 99) / 100) - 1];
        let mut benchmark = new_output(&output.join(OUTPUT_FILES[3]))?;
        writeln!(benchmark, "Adapter: {adapter:?}")?;
        writeln!(benchmark, "Backend: {:?}", adapter.backend)?;
        writeln!(
            benchmark,
            "Build: {}",
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }
        )?;
        writeln!(
            benchmark,
            "Scene: sample_world; weather: rain; walk phase: 0; frame: {frame}"
        )?;
        writeln!(
            benchmark,
            "Dimensions: {width}x{height} cells; SSAA factor: {SSAA_FACTOR}"
        )?;
        writeln!(
            benchmark,
            "Warmup frames: {WARMUP_FRAMES}; measured frames: {MEASURED_FRAMES}"
        )?;
        writeln!(
            benchmark,
            "Inputs held fixed for every warmup and measured frame; exact repeated cells verified."
        )?;
        writeln!(benchmark, "Timing: synchronous end-to-end scene clear/build, uploads, GPU render/compute, and completed CPU readback.")?;
        writeln!(benchmark, "Excludes GPU initialization, validation, JSON/ANSI/text serialization, file I/O, HUD, and terminal presentation.")?;
        writeln!(
            benchmark,
            "min_ms: {:.6}\nmedian_ms: {median:.6}\np95_ms: {p95:.6}",
            sorted[0]
        )?;
        writeln!(
            benchmark,
            "p95 method: nearest rank; median: mean of middle two samples"
        )?;
        writeln!(benchmark, "samples_ms (execution order): {timings_ms:?}")?;
        benchmark.flush()?;

        println!(
            "Captured {width}x{height} cells at frame {frame} using {} ({:?}).",
            adapter.name, adapter.backend
        );
        println!("End-to-end scene/readback: min {:.3} ms, median {median:.3} ms, p95 {p95:.3} ms; excludes terminal presentation.", sorted[0]);
        for name in OUTPUT_FILES {
            println!("{}", output.join(name).display());
        }
        Ok(())
    }

    fn parse_number<T: std::str::FromStr>(
        value: Option<OsString>,
        default: T,
        name: &str,
    ) -> Result<T, Box<dyn Error>> {
        match value {
            None => Ok(default),
            Some(value) => value
                .to_str()
                .and_then(|text| text.parse().ok())
                .ok_or_else(|| format!("invalid {name}: {value:?}").into()),
        }
    }

    fn prepare_output_dir(requested: &Path) -> Result<PathBuf, Box<dyn Error>> {
        let absolute = std::env::current_dir()?.join(requested);
        // Check the closest existing ancestor before create_dir_all, including file collisions.
        let mut ancestor = absolute.as_path();
        loop {
            match fs::metadata(ancestor) {
                Ok(metadata) if metadata.is_dir() => break,
                Ok(_) => {
                    return Err(
                        format!("output path is not a directory: {}", ancestor.display()).into(),
                    )
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    ancestor = ancestor
                        .parent()
                        .ok_or("output path has no existing parent")?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        for name in OUTPUT_FILES {
            let path = absolute.join(name);
            match fs::symlink_metadata(&path) {
                Ok(_) => {
                    return Err(format!(
                        "refusing to overwrite {}; choose a fresh output directory",
                        path.display()
                    )
                    .into())
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        fs::create_dir_all(&absolute)?;
        Ok(fs::canonicalize(absolute)?)
    }

    fn new_output(path: &Path) -> io::Result<BufWriter<fs::File>> {
        // create_new also prevents replacing a file/symlink created after the preflight check.
        Ok(BufWriter::new(
            OpenOptions::new().write(true).create_new(true).open(path)?,
        ))
    }

    fn validate_cells(cells: &[GpuCell], expected: usize) -> Result<(), Box<dyn Error>> {
        if cells.len() != expected {
            return Err(format!(
                "readback returned {} cells; expected {expected}",
                cells.len()
            )
            .into());
        }
        for (index, cell) in cells.iter().enumerate() {
            let valid_glyph = char::from_u32(cell.glyph).is_some_and(|glyph| !glyph.is_control());
            let valid_rgb = [
                cell.fg_r, cell.fg_g, cell.fg_b, cell.bg_r, cell.bg_g, cell.bg_b,
            ]
            .iter()
            .all(|channel| *channel <= 255);
            if !valid_glyph || !valid_rgb || !cell.depth().is_finite() {
                return Err(
                    format!("invalid GPU cell at row-major index {index}: {cell:?}").into(),
                );
            }
        }
        Ok(())
    }

    fn json_string(writer: &mut impl Write, value: &str) -> io::Result<()> {
        write!(writer, "\"")?;
        for ch in value.chars() {
            match ch {
                '"' => write!(writer, "\\\"")?,
                '\\' => write!(writer, "\\\\")?,
                '\u{0}'..='\u{1f}' => write!(writer, "\\u{:04x}", ch as u32)?,
                _ => write!(writer, "{ch}")?,
            }
        }
        write!(writer, "\"")
    }

    fn write_json(
        path: &Path,
        width: i16,
        height: i16,
        frame: u64,
        adapter: &wgpu::AdapterInfo,
        cells: &[GpuCell],
    ) -> io::Result<()> {
        let mut writer = new_output(path)?;
        write!(writer, "{{\n  \"width\": {width},\n  \"height\": {height},\n  \"frame\": {frame},\n  \"adapter\": ")?;
        json_string(&mut writer, &format!("{adapter:?}"))?;
        write!(writer, ",\n  \"backend\": ")?;
        json_string(&mut writer, &format!("{:?}", adapter.backend))?;
        writeln!(writer, ",\n  \"weather\": \"rain\",\n  \"scene\": \"sample_world\",\n  \"ssaa_factor\": {SSAA_FACTOR},\n  \"cells\": [")?;
        for (index, cell) in cells.iter().enumerate() {
            writeln!(writer, "    {{\"glyph\":{},\"fg\":[{},{},{}],\"bg\":[{},{},{}],\"depth\":{},\"depth_bits\":{}}}{}",
                cell.glyph, cell.fg_r, cell.fg_g, cell.fg_b, cell.bg_r, cell.bg_g, cell.bg_b,
                cell.depth(), cell.depth_bits, if index + 1 == cells.len() { "" } else { "," })?;
        }
        writeln!(writer, "  ]\n}}")?;
        writer.flush()
    }

    fn write_rows(path: &Path, cells: &[GpuCell], width: usize, ansi: bool) -> io::Result<()> {
        let mut writer = new_output(path)?;
        for row in cells.chunks_exact(width) {
            let mut last_colors: Option<(u32, u32, u32, u32, u32, u32)> = None;
            for cell in row {
                let colors = (
                    cell.fg_r, cell.fg_g, cell.fg_b, cell.bg_r, cell.bg_g, cell.bg_b,
                );
                if ansi && last_colors != Some(colors) {
                    write!(
                        writer,
                        "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m",
                        colors.0, colors.1, colors.2, colors.3, colors.4, colors.5
                    )?;
                    last_colors = Some(colors);
                }
                write!(
                    writer,
                    "{}",
                    char::from_u32(cell.glyph).expect("validated GPU glyph")
                )?;
            }
            if ansi {
                write!(writer, "\x1b[0m\r\n")?;
            } else {
                writeln!(writer)?;
            }
        }
        writer.flush()
    }
}
