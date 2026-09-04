//! The plugin's editor: a spectrum display on top of a grouped parameter list.
//! Built with `nih_plug_vizia`.

use nih_plug::prelude::Editor;
use nih_plug_vizia::vizia::prelude::*;
use nih_plug_vizia::vizia::vg;
use nih_plug_vizia::widgets::{GenericUi, ResizeHandle};
use nih_plug_vizia::{assets, create_vizia_editor, ViziaState, ViziaTheming};
use std::sync::Arc;

use crate::analyzer::{band_hz, AnalyzerBands, BANDS};
use crate::HarmonicSynthParams;

const WIDTH: u32 = 460;
const HEIGHT: u32 = 620;

#[derive(Lens)]
struct Data {
    params: Arc<HarmonicSynthParams>,
    bands: Arc<AnalyzerBands>,
}

impl Model for Data {}

pub(crate) fn default_state() -> Arc<ViziaState> {
    ViziaState::new(|| (WIDTH, HEIGHT))
}

pub(crate) fn create(
    params: Arc<HarmonicSynthParams>,
    bands: Arc<AnalyzerBands>,
    editor_state: Arc<ViziaState>,
) -> Option<Box<dyn Editor>> {
    create_vizia_editor(editor_state, ViziaTheming::Custom, move |cx, _| {
        assets::register_noto_sans_light(cx);
        assets::register_noto_sans_thin(cx);
        let _ = cx.add_stylesheet(STYLE);

        Data {
            params: params.clone(),
            bands: bands.clone(),
        }
        .build(cx);

        VStack::new(cx, |cx| {
            HStack::new(cx, |cx| {
                Label::new(cx, "HARMONIC SYNTH")
                    .font_family(vec![FamilyOwned::Name(String::from(assets::NOTO_SANS))])
                    .font_weight(FontWeightKeyword::Thin)
                    .font_size(22.0)
                    .top(Stretch(1.0))
                    .bottom(Stretch(1.0));
                Label::new(cx, env!("CARGO_PKG_VERSION"))
                    .class("dim")
                    .left(Stretch(1.0))
                    .top(Stretch(1.0))
                    .bottom(Stretch(1.0));
            })
            .class("titlebar")
            .height(Pixels(34.0))
            .col_between(Pixels(6.0));

            Spectrum::new(cx, Data::bands)
                .class("spectrum")
                .height(Pixels(120.0));

            ScrollView::new(cx, 0.0, 0.0, false, true, |cx| {
                GenericUi::new(cx, Data::params).width(Percentage(100.0));
            })
            .class("params")
            .width(Percentage(100.0));
        })
        .class("root");

        ResizeHandle::new(cx);
    })
}

/// A bar-graph spectrum view. Reads [`AnalyzerBands`] straight from the audio
/// thread every frame; the x-axis is log-frequency, the y-axis is dBFS-ish.
struct Spectrum {
    bands: Arc<AnalyzerBands>,
}

impl Spectrum {
    fn new<L: Lens<Target = Arc<AnalyzerBands>>>(cx: &mut Context, lens: L) -> Handle<'_, Self> {
        Self {
            bands: lens.get(cx),
        }
        .build(cx, |_| {})
    }
}

impl View for Spectrum {
    fn element(&self) -> Option<&'static str> {
        Some("spectrum")
    }

    fn draw(&self, cx: &mut DrawContext, canvas: &mut Canvas) {
        let b = cx.bounds();
        if b.w <= 0.0 || b.h <= 0.0 {
            return;
        }

        // gridlines at 100 Hz / 1 kHz / 10 kHz
        let (f_lo, f_hi) = (band_hz(0), band_hz(BANDS - 1));
        let x_of = |hz: f64| {
            let t = (hz / f_lo).ln() / (f_hi / f_lo).ln();
            b.x + b.w * t as f32
        };
        let grid = vg::Paint::color(vg::Color::rgba(255, 255, 255, 20));
        for &hz in &[100.0, 1_000.0, 10_000.0] {
            let x = x_of(hz);
            let mut p = vg::Path::new();
            p.move_to(x, b.y);
            p.line_to(x, b.y + b.h);
            canvas.stroke_path(&p, &grid);
        }

        // bars
        let fill = vg::Paint::color(cx.font_color().into());
        let bar_w = (b.w / BANDS as f32) * 0.72;
        for i in 0..BANDS {
            let mag = self.bands.get(i).max(1.0e-6);
            let db = 20.0 * mag.log10();
            // −72 dB … +6 dB mapped to the box height
            let h = (((db + 72.0) / 78.0).clamp(0.0, 1.0)) * b.h;
            let cx_px = x_of(band_hz(i));
            let mut p = vg::Path::new();
            p.rect(cx_px - bar_w * 0.5, b.y + b.h - h, bar_w, h);
            canvas.fill_path(&p, &fill);
        }
    }
}

const STYLE: &str = r#"
.root {
    background-color: #1b1d21;
    child-space: 8px;
    row-between: 8px;
}
.titlebar { color: #e8e8ea; }
.dim, label.dim { color: #6b7078; font-size: 11px; }
.spectrum {
    background-color: #111316;
    border-radius: 4px;
    border-width: 1px;
    border-color: #2a2d33;
    color: #6fae7a;
}
.params { background-color: #1b1d21; }
.params .row {
    height: 26px;
    col-between: 6px;
    child-top: 1s;
    child-bottom: 1s;
}
.params .label {
    width: 132px;
    color: #b9bcc2;
    font-size: 12px;
    text-align: right;
}
generic-ui { row-between: 3px; child-space: 2px; }
"#;
