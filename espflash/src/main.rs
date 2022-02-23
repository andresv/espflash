use std::{fs, mem::swap, path::PathBuf, str::FromStr};

use clap::{AppSettings, IntoApp, Parser};
use espflash::{
    cli::{
        board_info, connect, flash_elf_image, monitor::monitor, save_elf_as_image, ConnectOpts,
        FlashOpts,
    },
    Chip, Config, ImageFormatId,
};
use miette::{IntoDiagnostic, Result, WrapErr};

#[derive(Parser)]
#[clap(version, global_setting = AppSettings::PropagateVersion)]
struct Opts {
    /// Image format to flash
    #[clap(long)]
    pub format: Option<String>,
    #[clap(flatten)]
    flash_opts: FlashOpts,
    #[clap(flatten)]
    connect_opts: ConnectOpts,
    /// ELF image to flash
    image: Option<String>,
    #[clap(subcommand)]
    subcommand: Option<SubCommand>,
}

#[derive(Parser)]
pub enum SubCommand {
    /// Display information about the connected board and exit without flashing
    BoardInfo(ConnectOpts),
    /// Save the image to disk instead of flashing to device
    SaveImage(SaveImageOpts),
}

#[derive(Parser)]
pub struct SaveImageOpts {
    /// Image format to flash
    #[clap(long)]
    format: Option<String>,
    /// the chip to create an image for
    chip: Chip,
    /// ELF image to flash
    image: PathBuf,
    /// File name to save the generated image to
    file: PathBuf,
}

fn main() -> Result<()> {
    miette::set_panic_hook();

    let mut opts = Opts::parse();
    let config = Config::load()?;

    // If neither the IMAGE nor SERIAL arguments have been provided, print the help
    // message and exit.
    if opts.image.is_none() && opts.connect_opts.serial.is_none() {
        Opts::into_app().print_help().ok();
        return Ok(());
    }

    // If only a single argument is passed, it *should* always be the ELF file.
    // In the case that the serial port was not provided as a command-line argument,
    // we will either load the value specified in the configuration file or do port
    // auto-detection instead.
    if opts.image.is_none() && opts.connect_opts.serial.is_some() {
        swap(&mut opts.image, &mut opts.connect_opts.serial);
    }

    if let Some(subcommand) = opts.subcommand {
        use SubCommand::*;

        match subcommand {
            BoardInfo(opts) => board_info(opts, config),
            SaveImage(opts) => save_image(opts),
        }
    } else {
        flash(opts, config)
    }
}

fn flash(opts: Opts, config: Config) -> Result<()> {
    let mut flasher = connect(&opts.connect_opts, &config)?;
    flasher.board_info()?;

    let elf = if let Some(elf) = opts.image {
        elf
    } else {
        Opts::into_app().print_help().ok();
        return Ok(());
    };

    // Read the ELF data from the build path and load it to the target.
    let elf_data = fs::read(&elf).into_diagnostic()?;

    if opts.flash_opts.ram {
        flasher.load_elf_to_ram(&elf_data)?;
    } else {
        let bootloader = opts.flash_opts.bootloader.as_deref();
        let partition_table = opts.flash_opts.partition_table.as_deref();

        let image_format = opts
            .format
            .as_deref()
            .map(ImageFormatId::from_str)
            .transpose()?;

        flash_elf_image(
            &mut flasher,
            &elf_data,
            bootloader,
            partition_table,
            image_format,
        )?;
    }

    if opts.flash_opts.monitor {
        #[cfg(target_os = "linux")]
        {
            let (dtr, rts) = espflash::cli::create_dtr_rts_gpios_from_args(
                &opts.connect_opts.gpio_dtr,
                &opts.connect_opts.gpio_rts,
            )?;
            monitor(flasher.into_serial(), dtr, rts).into_diagnostic()?;
        }
        #[cfg(not(target_os = "linux"))]
        monitor(flasher.into_serial(), None, None).into_diagnostic()?;
    }

    Ok(())
}

fn save_image(opts: SaveImageOpts) -> Result<()> {
    let elf_data = fs::read(&opts.image)
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to open image {}", opts.image.display()))?;

    let image_format = opts
        .format
        .as_deref()
        .map(ImageFormatId::from_str)
        .transpose()?;

    save_elf_as_image(opts.chip, &elf_data, opts.file, image_format)?;

    Ok(())
}
