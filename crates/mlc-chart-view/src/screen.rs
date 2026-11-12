//! The phone screen around the chart, as a DOM tree on uzor's mobile widgets: the
//! top bar (symbol, venue, timeframe and whether the data is live or demo), the
//! timeframe segmented control, the chart slot, and a hint line over the home
//! indicator. Styled by `MOBILE_CSS` plus [`APP_CSS`] (greys only).

use uzor::ui::dom::El;
use uzor::ui::mobile::{segmented, top_bar, Dot};

use crate::demo::TIMEFRAMES;
use crate::i18n::{t, tf_label, K};

/// The chart's paint slot: its key and hit id (`El::paint_slot`; its rect comes
/// back from `Dom::slots`, its fingers with the slot frame).
pub const CHART_SLOT: &str = "chart";
/// Hit id of the reset button in the top bar.
pub const RESET_ID: &str = "nav:reset";

/// Rules on top of `MOBILE_CSS`.
pub const APP_CSS: &str = r#"
div.mlc-tf { flex-shrink: 0; padding: 4px 12px 8px; background: var(--m-bg) }
div.mlc-chart { flex-grow: 1; min-height: 120px; background: #121212; border-top: 1px solid var(--m-line); border-bottom: 1px solid var(--m-line) }
div.mlc-foot { flex-shrink: 0; padding: 8px 16px env(safe-area-inset-bottom, 0px); background: var(--m-bg); color: var(--m-dim); font-size: 12px; text-align: center; white-space: normal }
div.mlc-foot > div.mlc-hint { padding-bottom: 6px }
"#;

/// Reset icon (a circular arrow).
pub const ICON_RESET: &str = r#"<path d="M4 12a8 8 0 1 0 2.4-5.7M4 4v4h4"/>"#;

/// What the top bar says.
#[derive(Clone, Debug)]
pub struct Header<'a> {
    pub symbol: &'a str,
    pub venue: &'a str,
    pub live: bool,
}

/// The screen tree for timeframe `tf` (index into [`TIMEFRAMES`]).
pub fn tree(h: &Header<'_>, tf: usize) -> El {
    let status = format!("{} · {} · {}", h.venue, tf_label(TIMEFRAMES[tf].0), if h.live { t(K::Live) } else { t(K::Demo) });
    let dot = if h.live { Dot::Ok } else { Dot::Idle };
    let labels: Vec<String> = TIMEFRAMES.iter().map(|(c, _)| tf_label(c)).collect();
    let items: Vec<(&str, Option<u32>)> = labels.iter().map(|l| (l.as_str(), None)).collect();
    El::new("div.m-screen").css("height: 100%").child(El::new("div.m-safe-top")).children([
        top_bar("nav", t(K::Markets), h.symbol, Some((dot, &status)), &[("reset", ICON_RESET, t(K::Reset))]),
        El::new("div.mlc-tf").name(t(K::Timeframes)).child(segmented("tf", &items, tf)),
        El::new("div.mlc-chart").paint_slot(CHART_SLOT).name(t(K::Chart)),
        El::new("div.mlc-foot").child(El::new("div.mlc-hint").text(t(K::Hint))),
    ])
}

/// The timeframe index a click on `id` picks.
pub fn picked_tf(id: &str) -> Option<usize> {
    id.strip_prefix("tf:")?.parse().ok().filter(|i| *i < TIMEFRAMES.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uzor::ui::dom::lay_out_in;
    use uzor::ui::media::{Edges, MediaEnv};
    use uzor::ui::style::{Sheet, State};
    use uzor::Rect;

    pub(crate) fn phone_sheet(env: &MediaEnv) -> Sheet {
        let css = [uzor::ui::domkit::KIT_CSS, uzor::ui::domkit::KIT_TOUCH_CSS, uzor::ui::mobile::MOBILE_CSS, APP_CSS].concat();
        Sheet::parse_with(&css, env).expect("sheet parses")
    }

    fn inside(outer: Rect, r: Rect) -> bool {
        r.x >= outer.x - 0.5 && r.y >= outer.y - 0.5 && r.x + r.width <= outer.x + outer.width + 0.5 && r.y + r.height <= outer.y + outer.height + 0.5
    }

    #[test]
    fn phone_390x844_nothing_clipped_and_clear_of_the_notch_and_home_indicator() {
        for lang in [crate::i18n::Lang::Ru, crate::i18n::Lang::En] {
            crate::i18n::set_lang(lang);
            let env = MediaEnv::touch(390.0, 844.0, Edges::new(47.0, 0.0, 34.0, 0.0));
            let sheet = phone_sheet(&env);
            let mut ctx = uzor::testing::RecordingRenderContext::new();
            let screen = Rect::new(0.0, 0.0, 390.0, 844.0);
            for (live, tf) in [(false, 3), (true, 0), (false, 5)] {
                let root = tree(&Header { symbol: "BTC/USDT", venue: "Binance", live }, tf).css("height: 844px");
                let d = lay_out_in(&mut ctx, &sheet, &root, 0.0, 0.0, 390.0, Some(screen), &|_| State::NONE);
                let back = d.rect("nav:back").unwrap();
                let reset = d.rect(RESET_ID).expect("reset button");
                assert!(back.y >= 47.0 && reset.y >= 47.0, "top bar below the notch");
                assert!(inside(screen, reset) && reset.x + reset.width <= 390.0 - 8.0, "reset not clipped: {reset:?}");
                for i in 0..TIMEFRAMES.len() {
                    let b = d.rect(&format!("tf:{i}")).expect("tf button");
                    assert!(inside(screen, b) && b.width >= 40.0, "tf {i} fits: {b:?}");
                }
                let chart = d.slots().find(|(k, _)| *k == CHART_SLOT).map(|(_, r)| r).expect("chart slot");
                assert!(chart.y >= reset.y + reset.height && chart.height >= 450.0, "the chart gets most of the screen: {chart:?}");
                assert_eq!((chart.x, chart.width), (0.0, 390.0));
                assert!(d.touch_targets().any(|(id, _)| id == CHART_SLOT), "the chart captures its fingers");
                let foot = d.boxes.iter().find(|p| p.text.as_deref() == Some(t(K::Hint))).expect("hint");
                assert!(foot.rect.y >= chart.y + chart.height, "hint under the chart");
                assert!(foot.rect.y + foot.rect.height <= 844.0 - 34.0 + 0.5, "hint above the home indicator: {:?}", foot.rect);
                assert!(inside(screen, foot.rect));
                // Every text box keeps its text inside the screen.
                for p in d.boxes.iter().filter(|p| p.text.is_some()) {
                    assert!(inside(screen, p.rect), "{:?} clipped at {:?}", p.text, p.rect);
                }
            }
        }
        crate::i18n::set_lang(crate::i18n::Lang::Ru);
    }

    #[test]
    fn tf_ids() {
        assert_eq!(picked_tf("tf:3"), Some(3));
        assert_eq!(picked_tf("tf:9"), None);
        assert_eq!(picked_tf("nav:back"), None);
    }

    #[test]
    fn app_css_is_grey() {
        let mut rest = APP_CSS;
        while let Some(i) = rest.find('#') {
            let hex: String = rest[i + 1..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
            rest = &rest[i + 1..];
            if hex.len() == 6 {
                assert!(hex[0..2] == hex[2..4] && hex[2..4] == hex[4..6], "#{hex} not grey");
            }
        }
        for c in [crate::paint::colors::BG, crate::paint::colors::UP, crate::paint::colors::DOWN, crate::paint::colors::TEXT, crate::paint::colors::LABEL_BG] {
            assert!(c[1..3] == c[3..5] && c[3..5] == c[5..7], "{c} not grey");
        }
    }
}
