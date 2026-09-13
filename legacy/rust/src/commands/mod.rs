pub mod help;

pub use help::{
    command_registry, find_command, help_surface_report, palette_surface_report, render_help_text,
    CommandSpec, HelpSurfaceReport, PaletteEntry, PaletteSurfaceReport,
};
