use vignette_core::lattice::Rect;
use vignette_core::markdown::Block;

use crate::look::{AgentColor, BACKGROUND, BRIGHT, DIM, HEADER, HUE, RULE, Rgb, TEXT};
use crate::win32::{Canvas, Face, Painted};

/// One thing set out: what its window says and how it is marked.
pub struct Pane {
    pub title: String,
    pub mark: Option<String>,
    pub by: String,
    pub agent: Option<AgentColor>,
    pub blocks: Vec<Block>,
    pub scale: f64,
}

impl Pane {
    fn px(&self, points: f64) -> i32 {
        (points * self.scale).round() as i32
    }

    fn header(&self, canvas: &Canvas, width: i32) -> i32 {
        let tall = self.px(28.0);
        let line = self.px(18.0);
        let top = (tall - line) / 2;
        canvas.fill(Rect { x: 0, y: 0, w: width, h: tall }, HEADER);
        canvas.fill(Rect { x: 0, y: 0, w: width, h: self.px(2.0) }, HUE);
        let mut x = self.px(12.0);
        if let Some(agent) = self.agent {
            let side = self.px(9.0);
            canvas.fill(Rect { x, y: (tall - side) / 2 + 1, w: side, h: side }, agent.rgb());
            x += side + self.px(8.0);
        }
        let room = (width - x - self.px(12.0)).max(0);
        x += canvas.line(Rect { x, y: top, w: room, h: line }, &self.title, Face::Title, TEXT);
        if let Some(mark) = &self.mark {
            let pad = self.px(7.0);
            let wide = canvas.width_of(mark, Face::Title) + 2 * pad;
            x += self.px(10.0);
            if x + wide < width {
                canvas.fill(Rect { x, y: top - 1, w: wide, h: line + 2 }, HUE);
                canvas.line(Rect { x: x + pad, y: top, w: wide, h: line }, mark, Face::Title, BACKGROUND);
                x += wide;
            }
        }
        x += self.px(12.0);
        let left = width - x - self.px(12.0);
        if left > self.px(80.0) {
            canvas.line(Rect { x, y: top + 1, w: left, h: line }, &self.by, Face::Small, DIM);
        }
        tall
    }

    fn body(&self, canvas: &Canvas, top: i32, width: i32) {
        let pad = self.px(20.0);
        let inner = (width - 2 * pad).max(self.px(40.0));
        let mut y = top + self.px(14.0);
        for block in &self.blocks {
            y += match block {
                Block::Gap => self.px(8.0),
                Block::Heading(words) => self.heading(canvas, pad, y, inner, words),
                Block::Paragraph(words) => self.words(canvas, pad, y, inner, words, TEXT),
                Block::Bullet(words) => {
                    canvas.line(Rect { x: pad, y, w: pad, h: self.px(22.0) }, "\u{2022}", Face::Prose, DIM);
                    self.words(canvas, pad + self.px(18.0), y, inner - self.px(18.0), words, TEXT)
                }
                Block::Check { done, text } => self.check(canvas, pad, y, inner, *done, text),
                Block::Row { cells, head } => self.row(canvas, pad, y, inner, cells, *head),
            };
        }
    }

    fn heading(&self, canvas: &Canvas, x: i32, y: i32, width: i32, words: &str) -> i32 {
        let bar = self.px(4.0);
        let tall = canvas.height_of(width - bar - self.px(12.0), words, Face::Heading);
        canvas.fill(Rect { x, y: y + self.px(2.0), w: bar, h: tall - self.px(4.0) }, HUE);
        let at = Rect { x: x + bar + self.px(12.0), y, w: width - bar - self.px(12.0), h: tall };
        canvas.wrapped(at, words, Face::Heading, BRIGHT) + self.px(6.0)
    }

    fn words(&self, canvas: &Canvas, x: i32, y: i32, width: i32, words: &str, colour: Rgb) -> i32 {
        let tall = canvas.height_of(width, words, Face::Prose);
        canvas.wrapped(Rect { x, y, w: width, h: tall }, words, Face::Prose, colour) + self.px(4.0)
    }

    fn check(&self, canvas: &Canvas, x: i32, y: i32, width: i32, done: bool, words: &str) -> i32 {
        let side = self.px(13.0);
        let top = y + self.px(5.0);
        canvas.fill(Rect { x, y: top, w: side, h: side }, DIM);
        let inset = self.px(1.0).max(1);
        let inner = Rect { x: x + inset, y: top + inset, w: side - 2 * inset, h: side - 2 * inset };
        canvas.fill(inner, if done { HUE } else { BACKGROUND });
        let colour = if done { DIM } else { TEXT };
        self.words(canvas, x + side + self.px(10.0), y, width - side - self.px(10.0), words, colour)
    }

    fn row(&self, canvas: &Canvas, x: i32, y: i32, width: i32, cells: &[String], head: bool) -> i32 {
        let each = (width / cells.len().max(1) as i32).min(self.px(180.0));
        let tall = self.px(26.0);
        for (at, cell) in cells.iter().enumerate() {
            let place =
                Rect { x: x + at as i32 * each, y: y + self.px(4.0), w: each - self.px(8.0), h: tall };
            let (face, colour) = if head { (Face::Small, DIM) } else { (Face::Prose, TEXT) };
            canvas.line(place, cell, face, colour);
        }
        let ruled = each * cells.len() as i32;
        canvas.fill(Rect { x, y: y + tall + self.px(2.0), w: ruled, h: 1 }, RULE);
        tall + self.px(3.0)
    }
}

impl Painted for Pane {
    fn paint(&self, canvas: &Canvas, width: i32, height: i32) {
        canvas.fill(Rect { x: 0, y: 0, w: width, h: height }, BACKGROUND);
        let top = self.header(canvas, width);
        self.body(canvas, top, width);
    }
}
