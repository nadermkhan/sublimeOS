use alloc::vec;
use alloc::vec::Vec;

/// Coordinates in TrueType units (UPEM = 1000, baseline = 0, cap height = 720, x-height = 520, descender = -200).
#[derive(Clone, Copy)]
struct Pt {
    x: i16,
    y: i16,
    on: bool,
}

struct GlyphBuilder {
    contours: Vec<Vec<Pt>>,
    advance: u16,
    lsb: i16,
}

impl GlyphBuilder {
    fn new(advance: u16) -> Self {
        Self {
            contours: Vec::new(),
            advance,
            lsb: 50,
        }
    }

    /// Add an axis-aligned solid clockwise rectangle `[x0..x1] x [y0..y1]`.
    fn rect(&mut self, x0: i16, y0: i16, x1: i16, y1: i16) {
        self.contours.push(vec![
            Pt { x: x0, y: y0, on: true },
            Pt { x: x0, y: y1, on: true },
            Pt { x: x1, y: y1, on: true },
            Pt { x: x1, y: y0, on: true },
        ]);
    }

    /// Add a thick diagonal stroke between `(x0, y0)` and `(x1, y1)` with half-width `r` (CW).
    fn stroke(&mut self, x0: i16, y0: i16, x1: i16, y1: i16, r: i16) {
        let dx = (x1 - x0) as i32;
        let dy = (y1 - y0) as i32;
        if dx == 0 {
            let (ya, yb) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
            self.rect(x0 - r, ya, x0 + r, yb);
            return;
        }
        if dy == 0 {
            let (xa, xb) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
            self.rect(xa, y0 - r, xb, y0 + r);
            return;
        }
        // Horizontal bevel offsets give uniform optical stroke thickness across diagonals
        let nx = if dy.abs() >= dx.abs() {
            r
        } else {
            ((r as i32 * 5) / 4) as i16
        };
        if dx.abs() >= dy.abs() {
            // Mostly horizontal slanted bar: extend +/- ny vertically
            let (ax, ay, bx, by) = if x0 <= x1 { (x0, y0, x1, y1) } else { (x1, y1, x0, y0) };
            self.contours.push(vec![
                Pt { x: ax, y: ay - r, on: true },
                Pt { x: ax, y: ay + r, on: true },
                Pt { x: bx, y: by + r, on: true },
                Pt { x: bx, y: by - r, on: true },
            ]);
        } else {
            let (ax, ay, bx, by) = if y0 <= y1 { (x0, y0, x1, y1) } else { (x1, y1, x0, y0) };
            self.contours.push(vec![
                Pt { x: ax - nx, y: ay, on: true },
                Pt { x: bx - nx, y: by, on: true },
                Pt { x: bx + nx, y: by, on: true },
                Pt { x: ax + nx, y: ay, on: true },
            ]);
        }
    }

    /// Add an elliptical ring (outer CW, optional inner hole CCW) centered at `(cx, cy)`.
    fn ellipse(&mut self, cx: i16, cy: i16, rx: i16, ry: i16, inner_rx: i16, inner_ry: i16) {
        self.contours.push(oval_contour(cx, cy, rx, ry, true));
        if inner_rx > 0 && inner_ry > 0 {
            self.contours.push(oval_contour(cx, cy, inner_rx, inner_ry, false));
        }
    }

    /// Add a dot centered at `(cx, cy)` with radius `r`.
    fn dot(&mut self, cx: i16, cy: i16, r: i16) {
        self.contours.push(oval_contour(cx, cy, r, r, true));
    }

    /// Add a left-facing C-bowl (`[`-curved open on the right) in `[x0..x1] x [y0..y1]` with thickness `t`.
    fn bowl_c(&mut self, x0: i16, y0: i16, x1: i16, y1: i16, t: i16) {
        let mid_y = (y0 + y1) / 2;
        let rx = (x1 - x0).max(t + 20);
        self.contours.push(vec![
            Pt { x: x1, y: y1, on: true },
            Pt { x: x0, y: y1, on: false },
            Pt { x: x0, y: mid_y, on: true },
            Pt { x: x0, y: y0, on: false },
            Pt { x: x1, y: y0, on: true },
            Pt { x: x1, y: y0 + t, on: true },
            Pt { x: x0 + (t * 6 / 5).min(rx - 10), y: y0 + t, on: false },
            Pt { x: x0 + (t * 6 / 5).min(rx - 10), y: mid_y, on: true },
            Pt { x: x0 + (t * 6 / 5).min(rx - 10), y: y1 - t, on: false },
            Pt { x: x1, y: y1 - t, on: true },
        ]);
    }

    /// Add a right-facing reverse-C bowl (open on the left!) in `[x0..x1] x [y0..y1]` with thickness `t`.
    fn bowl_c_right(&mut self, x0: i16, y0: i16, x1: i16, y1: i16, t: i16) {
        let mid_y = (y0 + y1) / 2;
        let rx = (x1 - x0).max(t + 20);
        let in_x = x1 - (t * 6 / 5).min(rx - 10);
        self.contours.push(vec![
            Pt { x: x0, y: y1, on: true },
            Pt { x: x1, y: y1, on: false },
            Pt { x: x1, y: mid_y, on: true },
            Pt { x: x1, y: y0, on: false },
            Pt { x: x0, y: y0, on: true },
            Pt { x: x0, y: y0 + t, on: true },
            Pt { x: in_x, y: y0 + t, on: false },
            Pt { x: in_x, y: mid_y, on: true },
            Pt { x: in_x, y: y1 - t, on: false },
            Pt { x: x0, y: y1 - t, on: true },
        ]);
    }

    /// Add a clean sans-serif S curve fitting inside `[x0..x1] x [y0..y1]` with stroke thickness `t`.
    fn s_glyph(&mut self, x0: i16, y0: i16, x1: i16, y1: i16, t: i16) {
        let mid_y = (y0 + y1) / 2;
        let half_t = t / 2;
        let mid_hi = mid_y + half_t;
        let mid_lo = mid_y - half_t;
        // Top open C loop (`[x0..x1-10] x [mid_lo..y1]`) + bottom reverse-C loop (`[x0..x1] x [y0..mid_hi]`)
        // Wait: using one continuous smooth S polygon guarantees zero seam notch at the waist!
        let rx = (x1 - x0).max(t + 40);
        let in_r = (t * 6 / 5).min(rx / 2 - 8);
        let x_left_in = x0 + in_r;
        let x_right_in = x1 - in_r;
        let x_mid = (x0 + x1) / 2;
        self.contours.push(vec![
            // Start top-right terminal, sweep top -> upper-left -> waist -> lower-right -> bottom-left (outer boundary)
            Pt { x: x1 - 24, y: y1 - 36, on: true },
            Pt { x: x1 - 45, y: y1, on: false },
            Pt { x: x_mid, y: y1, on: true },
            Pt { x: x0, y: y1, on: false },
            Pt { x: x0 + 8, y: (y1 + mid_y) / 2, on: true },
            Pt { x: x0 + 16, y: mid_lo, on: false },
            Pt { x: x_mid, y: mid_lo - 10, on: true },
            Pt { x: x_right_in, y: mid_lo - 22, on: false },
            Pt { x: x_right_in + 6, y: (y0 + mid_y) / 2, on: true },
            Pt { x: x_right_in, y: y0 + t, on: false },
            Pt { x: x_mid - 10, y: y0 + t, on: true },
            Pt { x: x0 + 55, y: y0 + t, on: false },
            Pt { x: x0 + 18, y: y0 + t + 32, on: true },
            // Bottom-left terminal drop
            Pt { x: x0, y: y0 + 50, on: true },
            Pt { x: x0 + 40, y: y0, on: false },
            Pt { x: x_mid, y: y0, on: true },
            Pt { x: x1, y: y0, on: false },
            Pt { x: x1, y: (y0 + mid_y) / 2, on: true },
            Pt { x: x1, y: mid_hi, on: false },
            Pt { x: x_mid + 10, y: mid_hi + 8, on: true },
            Pt { x: x_left_in - 6, y: mid_hi + 16, on: false },
            Pt { x: x_left_in - 6, y: (y1 + mid_y) / 2, on: true },
            Pt { x: x_left_in, y: y1 - t, on: false },
            Pt { x: x_mid, y: y1 - t, on: true },
            Pt { x: x1 - 65, y: y1 - t, on: false },
            Pt { x: x1 - 36, y: y1 - t - 26, on: true },
        ]);
    }

    /// Add a U-cup curve open at the top `[x0..x1] x [y0..y1]` with stroke thickness `t`.
    fn bowl_u(&mut self, x0: i16, y0: i16, x1: i16, y1: i16, t: i16) {
        let mid_x = (x0 + x1) / 2;
        let k = (y1 - y0).min(240);
        self.contours.push(vec![
            Pt { x: x0, y: y1, on: true },
            Pt { x: x0, y: y0 + k / 2, on: true },
            Pt { x: x0, y: y0, on: false },
            Pt { x: mid_x, y: y0, on: true },
            Pt { x: x1, y: y0, on: false },
            Pt { x: x1, y: y0 + k / 2, on: true },
            Pt { x: x1, y: y1, on: true },
            Pt { x: x1 - t, y: y1, on: true },
            Pt { x: x1 - t, y: y0 + k / 2, on: true },
            Pt { x: x1 - t, y: y0 + t, on: false },
            Pt { x: mid_x, y: y0 + t, on: true },
            Pt { x: x0 + t, y: y0 + t, on: false },
            Pt { x: x0 + t, y: y0 + k / 2, on: true },
            Pt { x: x0 + t, y: y1, on: true },
        ]);
    }

    /// Add a lower-case arch `n`/`m`/`h` open at bottom `[x0..x1] x [y0..y1]`.
    fn arch_n(&mut self, x0: i16, y0: i16, x1: i16, y1: i16, t: i16) {
        let mid_x = (x0 + x1) / 2;
        let k = (y1 - y0).min(220);
        self.contours.push(vec![
            Pt { x: x0, y: y0, on: true },
            Pt { x: x0, y: y1 - k / 2, on: true },
            Pt { x: x0, y: y1, on: false },
            Pt { x: mid_x, y: y1, on: true },
            Pt { x: x1, y: y1, on: false },
            Pt { x: x1, y: y1 - k / 2, on: true },
            Pt { x: x1, y: y0, on: true },
            Pt { x: x1 - t, y: y0, on: true },
            Pt { x: x1 - t, y: y1 - k / 2, on: true },
            Pt { x: x1 - t, y: y1 - t, on: false },
            Pt { x: mid_x, y: y1 - t, on: true },
            Pt { x: x0 + t, y: y1 - t, on: false },
            Pt { x: x0 + t, y: y1 - k / 2, on: true },
            Pt { x: x0 + t, y: y0, on: true },
        ]);
    }

    /// Add a D-bowl (closed on left `x_left`, rounded right up to `x_right`).
    fn bowl_d(&mut self, x_left: i16, x_right: i16, y0: i16, y1: i16, t: i16) {
        let mid_y = (y0 + y1) / 2;
        self.contours.push(vec![
            Pt { x: x_left, y: y0, on: true },
            Pt { x: x_left, y: y1, on: true },
            Pt { x: x_right, y: y1, on: false },
            Pt { x: x_right, y: mid_y, on: true },
            Pt { x: x_right, y: y0, on: false },
        ]);
        if y1 - y0 > 2 * t + 40 && x_right - x_left > 2 * t + 40 {
            self.contours.push(vec![
                Pt { x: x_left + t, y: y0 + t, on: true },
                Pt { x: x_right - t, y: y0 + t, on: false },
                Pt { x: x_right - t, y: mid_y, on: true },
                Pt { x: x_right - t, y: y1 - t, on: false },
                Pt { x: x_left + t, y: y1 - t, on: true },
            ]);
        }
    }
}

fn oval_contour(cx: i16, cy: i16, rx: i16, ry: i16, cw: bool) -> Vec<Pt> {
    // 8-point TrueType quadratic ellipse approximation (4 on-curve + 4 off-curve)
    let pts = [
        Pt { x: cx - rx, y: cy, on: true },
        Pt { x: cx - rx, y: cy + ry, on: false },
        Pt { x: cx, y: cy + ry, on: true },
        Pt { x: cx + rx, y: cy + ry, on: false },
        Pt { x: cx + rx, y: cy, on: true },
        Pt { x: cx + rx, y: cy - ry, on: false },
        Pt { x: cx, y: cy - ry, on: true },
        Pt { x: cx - rx, y: cy - ry, on: false },
    ];
    if cw {
        pts.to_vec()
    } else {
        pts.into_iter().rev().collect()
    }
}

fn build_ascii_glyph(ch: u8) -> GlyphBuilder {
    const W: i16 = 82; // Primary sans stroke thickness in 1000-UPEM space
    const CAP: i16 = 720;
    const XH: i16 = 520;
    let mut g = GlyphBuilder::new(560);

    match ch {
        b' ' => {
            g.advance = 300;
            g.lsb = 0;
        }
        b'!' => {
            g.advance = 300;
            g.rect(110, 190, 190, CAP);
            g.dot(150, 55, 50);
        }
        b'"' => {
            g.advance = 400;
            g.rect(90, 490, 165, CAP);
            g.rect(235, 490, 310, CAP);
        }
        b'#' => {
            g.advance = 600;
            g.stroke(170, 0, 230, CAP, 38);
            g.stroke(370, 0, 430, CAP, 38);
            g.rect(70, 210, 530, 285);
            g.rect(85, 435, 545, 510);
        }
        b'$' => {
            g.advance = 560;
            g = build_ascii_glyph(b'S');
            g.rect(250, -60, 310, 780);
        }
        b'%' => {
            g.advance = 700;
            g.ellipse(170, 560, 110, 130, 50, 70);
            g.ellipse(530, 140, 110, 130, 50, 70);
            g.stroke(140, 0, 560, CAP, 36);
        }
        b'&' => {
            g.advance = 620;
            g.ellipse(260, 540, 140, 160, 68, 92);
            g.ellipse(270, 190, 190, 190, 115, 115);
            g.stroke(140, 380, 540, 0, 42);
        }
        b'\'' => {
            g.advance = 240;
            g.rect(85, 490, 160, CAP);
        }
        b'(' => {
            g.advance = 340;
            g.bowl_c(90, -140, 290, 760, 72);
        }
        b')' => {
            g.advance = 340;
            // Mirrored C
            g.contours.push(vec![
                Pt { x: 50, y: 760, on: true },
                Pt { x: 250, y: 760, on: false },
                Pt { x: 250, y: 310, on: true },
                Pt { x: 250, y: -140, on: false },
                Pt { x: 50, y: -140, on: true },
                Pt { x: 50, y: -68, on: true },
                Pt { x: 175, y: -68, on: false },
                Pt { x: 175, y: 310, on: true },
                Pt { x: 175, y: 688, on: false },
                Pt { x: 50, y: 688, on: true },
            ]);
        }
        b'*' => {
            g.advance = 460;
            g.rect(198, 410, 262, 690);
            g.stroke(100, 470, 360, 630, 32);
            g.stroke(100, 630, 360, 470, 32);
        }
        b'+' => {
            g.advance = 560;
            g.rect(240, 130, 320, 550);
            g.rect(70, 300, 490, 380);
        }
        b',' => {
            g.advance = 260;
            g.rect(90, 0, 175, 110);
            g.stroke(135, 40, 80, -120, 34);
        }
        b'-' => {
            g.advance = 360;
            g.rect(60, 270, 300, 345);
        }
        b'.' => {
            g.advance = 260;
            g.dot(130, 50, 50);
        }
        b'/' => {
            g.advance = 420;
            g.stroke(70, -60, 350, CAP + 20, 40);
        }
        b'0' => {
            g.advance = 560;
            g.ellipse(280, 360, 220, 360, 138, 278);
            g.stroke(150, 120, 410, 600, 28);
        }
        b'1' => {
            g.advance = 520;
            g.rect(240, 0, 240 + W, CAP);
            g.rect(120, 0, 420, 76);
            g.stroke(120, 550, 280, CAP, 38);
        }
        b'2' => {
            g.advance = 560;
            g.contours.push(vec![
                Pt { x: 80, y: 540, on: true },
                Pt { x: 80, y: 720, on: false },
                Pt { x: 280, y: 720, on: true },
                Pt { x: 490, y: 720, on: false },
                Pt { x: 490, y: 520, on: true },
                Pt { x: 490, y: 380, on: false },
                Pt { x: 185, y: 80, on: true },
                Pt { x: 495, y: 80, on: true },
                Pt { x: 495, y: 0, on: true },
                Pt { x: 70, y: 0, on: true },
                Pt { x: 70, y: 78, on: true },
                Pt { x: 405, y: 390, on: false },
                Pt { x: 405, y: 520, on: true },
                Pt { x: 405, y: 640, on: false },
                Pt { x: 280, y: 640, on: true },
                Pt { x: 162, y: 640, on: false },
                Pt { x: 162, y: 540, on: true },
            ]);
        }
        b'3' => {
            g.advance = 560;
            g.bowl_c_right(95, 330, 485, CAP, 78);
            g.bowl_c_right(85, 0, 500, 405, 78);
            g.rect(190, 330, 310, 405);
        }
        b'4' => {
            g.advance = 560;
            g.rect(340, 0, 340 + W, CAP);
            g.rect(70, 200, 500, 278);
            g.stroke(90, 240, 380, CAP, 40);
        }
        b'5' => {
            g.advance = 560;
            g.rect(110, 640, 470, CAP);
            g.rect(110, 380, 192, CAP);
            g.bowl_c_right(95, 0, 505, 455, 80);
        }
        b'6' => {
            g.advance = 560;
            g.ellipse(285, 225, 215, 225, 132, 145);
            g.bowl_c(70, 180, 460, CAP, 80);
        }
        b'7' => {
            g.advance = 560;
            g.rect(70, 638, 490, CAP);
            g.stroke(170, 0, 450, 680, 42);
        }
        b'8' => {
            g.advance = 560;
            g.ellipse(280, 530, 195, 190, 115, 115);
            g.ellipse(280, 205, 220, 205, 138, 130);
        }
        b'9' => {
            g.advance = 560;
            g.ellipse(275, 495, 215, 225, 132, 145);
            g.rect(408, 130, 490, 520);
            g.rect(120, 0, 460, 80);
        }
        b':' => {
            g.advance = 280;
            g.dot(140, 440, 50);
            g.dot(140, 50, 50);
        }
        b';' => {
            g.advance = 280;
            g.dot(140, 440, 50);
            g.rect(100, 0, 185, 110);
            g.stroke(145, 40, 90, -120, 34);
        }
        b'<' => {
            g.advance = 540;
            g.stroke(100, 330, 450, 580, 38);
            g.stroke(100, 330, 450, 80, 38);
        }
        b'=' => {
            g.advance = 560;
            g.rect(75, 415, 485, 490);
            g.rect(75, 215, 485, 290);
        }
        b'>' => {
            g.advance = 540;
            g.stroke(440, 330, 90, 580, 38);
            g.stroke(440, 330, 90, 80, 38);
        }
        b'?' => {
            g.advance = 520;
            g.bowl_c_right(110, 340, 455, CAP, 78);
            g.rect(210, 205, 290, 395);
            g.dot(250, 55, 50);
        }
        b'@' => {
            g.advance = 760;
            g.ellipse(380, 340, 320, 360, 240, 280);
            g.ellipse(380, 340, 155, 175, 80, 98);
        }
        b'A' => {
            g.advance = 640;
            g.stroke(80, 0, 320, CAP, 44);
            g.stroke(560, 0, 320, CAP, 44);
            g.rect(175, 210, 465, 290);
        }
        b'B' => {
            g.advance = 600;
            g.rect(80, 0, 80 + W, CAP);
            g.bowl_d(80, 510, 335, CAP, 80);
            g.bowl_d(80, 535, 0, 415, 80);
        }
        b'C' => {
            g.advance = 600;
            g.bowl_c(65, 0, 520, CAP, W);
        }
        b'D' => {
            g.advance = 640;
            g.bowl_d(85, 575, 0, CAP, W);
        }
        b'E' => {
            g.advance = 560;
            g.rect(85, 0, 85 + W, CAP);
            g.rect(85, CAP - W, 495, CAP);
            g.rect(85, 330, 445, 330 + 76);
            g.rect(85, 0, 505, W);
        }
        b'F' => {
            g.advance = 540;
            g.rect(85, 0, 85 + W, CAP);
            g.rect(85, CAP - W, 485, CAP);
            g.rect(85, 330, 435, 330 + 76);
        }
        b'G' => {
            g.advance = 640;
            g.bowl_c(65, 0, 540, CAP, W);
            g.rect(350, 280, 550, 360);
            g.rect(470, 0, 550, 360);
        }
        b'H' => {
            g.advance = 640;
            g.rect(85, 0, 85 + W, CAP);
            g.rect(475, 0, 475 + W, CAP);
            g.rect(85, 330, 555, 330 + 78);
        }
        b'I' => {
            g.advance = 300;
            g.rect(108, 0, 192, CAP);
        }
        b'J' => {
            g.advance = 480;
            g.bowl_u(60, 0, 395, 250, 80);
            g.rect(313, 180, 395, CAP);
        }
        b'K' => {
            g.advance = 600;
            g.rect(85, 0, 85 + W, CAP);
            g.stroke(140, 320, 515, CAP, 44);
            g.stroke(240, 405, 535, 0, 44);
        }
        b'L' => {
            g.advance = 520;
            g.rect(85, 0, 85 + W, CAP);
            g.rect(85, 0, 470, W);
        }
        b'M' => {
            g.advance = 760;
            g.rect(75, 0, 75 + W, CAP);
            g.rect(685 - W, 0, 685, CAP);
            g.stroke(115, CAP, 380, 140, 42);
            g.stroke(645, CAP, 380, 140, 42);
        }
        b'N' => {
            g.advance = 640;
            g.rect(85, 0, 85 + W, CAP);
            g.rect(555 - W, 0, 555, CAP);
            g.stroke(125, CAP, 515, 0, 44);
        }
        b'O' => {
            g.advance = 680;
            g.ellipse(340, 360, 275, 360, 192, 278);
        }
        b'P' => {
            g.advance = 580;
            g.rect(85, 0, 85 + W, CAP);
            g.bowl_d(85, 520, 310, CAP, 80);
        }
        b'Q' => {
            g.advance = 680;
            g.ellipse(340, 360, 275, 360, 192, 278);
            g.stroke(390, 170, 600, -70, 42);
        }
        b'R' => {
            g.advance = 620;
            g.rect(85, 0, 85 + W, CAP);
            g.bowl_d(85, 525, 320, CAP, 80);
            g.stroke(330, 360, 550, 0, 44);
        }
        b'S' => {
            g.advance = 560;
            g.s_glyph(68, 0, 492, CAP, 80);
        }
        b'T' => {
            g.advance = 580;
            g.rect(45, CAP - W, 535, CAP);
            g.rect(249, 0, 249 + W, CAP);
        }
        b'U' => {
            g.advance = 640;
            g.bowl_u(85, 0, 555, CAP, W);
        }
        b'V' => {
            g.advance = 620;
            g.stroke(80, CAP, 310, 0, 44);
            g.stroke(540, CAP, 310, 0, 44);
        }
        b'W' => {
            g.advance = 840;
            g.stroke(65, CAP, 240, 0, 40);
            g.stroke(240, 0, 420, 470, 38);
            g.stroke(420, 470, 600, 0, 38);
            g.stroke(775, CAP, 600, 0, 40);
        }
        b'X' => {
            g.advance = 600;
            g.stroke(80, 0, 520, CAP, 44);
            g.stroke(80, CAP, 520, 0, 44);
        }
        b'Y' => {
            g.advance = 580;
            g.stroke(65, CAP, 290, 320, 42);
            g.stroke(515, CAP, 290, 320, 42);
            g.rect(249, 0, 249 + W, 360);
        }
        b'Z' => {
            g.advance = 580;
            g.rect(65, CAP - W, 515, CAP);
            g.rect(65, 0, 515, W);
            g.stroke(70, 40, 505, CAP - 40, 42);
        }
        b'[' => {
            g.advance = 300;
            g.rect(90, -130, 162, 750);
            g.rect(90, 682, 260, 750);
            g.rect(90, -130, 260, -62);
        }
        b'\\' => {
            g.advance = 420;
            g.stroke(70, CAP + 20, 350, -60, 40);
        }
        b']' => {
            g.advance = 300;
            g.rect(138, -130, 210, 750);
            g.rect(40, 682, 210, 750);
            g.rect(40, -130, 210, -62);
        }
        b'^' => {
            g.advance = 480;
            g.stroke(90, 450, 240, 700, 36);
            g.stroke(390, 450, 240, 700, 36);
        }
        b'_' => {
            g.advance = 520;
            g.rect(20, -110, 500, -45);
        }
        b'`' => {
            g.advance = 320;
            g.stroke(110, 690, 210, 540, 34);
        }
        b'a' => {
            g.advance = 520;
            g.bowl_c(68, 0, 420, 295, 72);
            g.rect(368, 0, 444, 375);
            g.arch_n(110, 315, 444, XH, 74);
        }
        b'b' => {
            g.advance = 540;
            g.rect(80, 0, 80 + 78, CAP);
            g.ellipse(285, 260, 200, 260, 120, 180);
        }
        b'c' => {
            g.advance = 480;
            g.bowl_c(60, 0, 435, XH, 78);
        }
        b'd' => {
            g.advance = 540;
            g.rect(460 - 78, 0, 460, CAP);
            g.ellipse(255, 260, 200, 260, 120, 180);
        }
        b'e' => {
            g.advance = 510;
            g.bowl_c(58, 0, 445, XH, 74);
            g.rect(98, 228, 448, 298);
            g.bowl_c_right(265, 228, 448, XH, 72);
        }
        b'f' => {
            g.advance = 340;
            g.rect(125, 0, 203, 590);
            g.rect(45, 440, 290, XH);
            g.rect(125, 590, 305, CAP);
        }
        b'g' => {
            g.advance = 540;
            g.ellipse(255, 260, 195, 260, 118, 182);
            g.rect(382, -135, 460, XH);
            g.rect(110, -210, 460, -135);
        }
        b'h' => {
            g.advance = 540;
            g.rect(80, 0, 158, CAP);
            g.arch_n(80, 0, 465, XH, 78);
        }
        b'i' => {
            g.advance = 250;
            g.rect(86, 0, 164, XH);
            g.dot(125, 650, 46);
        }
        b'j' => {
            g.advance = 260;
            g.rect(115, -130, 193, XH);
            g.rect(20, -205, 193, -130);
            g.dot(154, 650, 46);
        }
        b'k' => {
            g.advance = 490;
            g.rect(80, 0, 158, CAP);
            g.stroke(140, 210, 435, XH, 40);
            g.stroke(220, 290, 450, 0, 40);
        }
        b'l' => {
            g.advance = 250;
            g.rect(86, 0, 164, CAP);
        }
        b'm' => {
            g.advance = 780;
            g.arch_n(75, 0, 425, XH, 76);
            g.arch_n(350, 0, 705, XH, 76);
        }
        b'n' => {
            g.advance = 540;
            g.arch_n(80, 0, 465, XH, 78);
        }
        b'o' => {
            g.advance = 540;
            g.ellipse(270, 260, 210, 260, 130, 182);
        }
        b'p' => {
            g.advance = 540;
            g.rect(80, -210, 158, XH);
            g.ellipse(285, 260, 200, 260, 120, 180);
        }
        b'q' => {
            g.advance = 540;
            g.rect(382, -210, 460, XH);
            g.ellipse(255, 260, 200, 260, 120, 180);
        }
        b'r' => {
            g.advance = 360;
            g.rect(80, 0, 158, XH);
            g.rect(80, XH - 80, 320, XH);
        }
        b's' => {
            g.advance = 470;
            g.s_glyph(58, 0, 412, XH, 74);
        }
        b't' => {
            g.advance = 340;
            g.rect(120, 75, 198, 670);
            g.rect(45, 445, 295, XH);
            g.rect(120, 0, 305, 76);
        }
        b'u' => {
            g.advance = 540;
            g.bowl_u(75, 0, 460, XH, 78);
        }
        b'v' => {
            g.advance = 500;
            g.stroke(65, XH, 250, 0, 40);
            g.stroke(435, XH, 250, 0, 40);
        }
        b'w' => {
            g.advance = 720;
            g.stroke(60, XH, 200, 0, 38);
            g.stroke(200, 0, 360, 360, 36);
            g.stroke(360, 360, 520, 0, 36);
            g.stroke(660, XH, 520, 0, 38);
        }
        b'x' => {
            g.advance = 490;
            g.stroke(70, 0, 420, XH, 40);
            g.stroke(70, XH, 420, 0, 40);
        }
        b'y' => {
            g.advance = 500;
            g.stroke(70, XH, 260, 0, 40);
            g.stroke(430, XH, 165, -200, 40);
        }
        b'z' => {
            g.advance = 480;
            g.rect(60, XH - 76, 420, XH);
            g.rect(60, 0, 420, 76);
            g.stroke(70, 40, 410, XH - 40, 40);
        }
        b'{' => {
            g.advance = 340;
            g.bowl_c(125, -130, 290, 750, 70);
            g.rect(50, 275, 160, 345);
        }
        b'|' => {
            g.advance = 260;
            g.rect(96, -150, 164, 770);
        }
        b'}' => {
            g.advance = 340;
            let mirror = build_ascii_glyph(b')');
            g.contours = mirror.contours;
            g.rect(180, 275, 290, 345);
        }
        b'~' => {
            g.advance = 540;
            g.stroke(65, 310, 210, 420, 34);
            g.stroke(210, 420, 330, 300, 34);
            g.stroke(330, 300, 475, 410, 34);
        }
        _ => {
            // Empty/notdef box
            g.rect(70, 0, 480, CAP);
            g.contours.push(vec![
                Pt { x: 145, y: 75, on: true },
                Pt { x: 405, y: 75, on: true },
                Pt { x: 405, y: CAP - 75, on: true },
                Pt { x: 145, y: CAP - 75, on: true },
            ]);
        }
    }
    g
}

fn push_be16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn push_be16_i(out: &mut Vec<u8>, v: i16) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn push_be32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn pad4(out: &mut Vec<u8>) {
    while (out.len() & 3) != 0 {
        out.push(0);
    }
}

fn serialize_glyph(g: &GlyphBuilder) -> Vec<u8> {
    if g.contours.is_empty() {
        return Vec::new();
    }
    let mut x_min = i16::MAX;
    let mut y_min = i16::MAX;
    let mut x_max = i16::MIN;
    let mut y_max = i16::MIN;
    let mut total_pts = 0usize;
    for c in &g.contours {
        total_pts += c.len();
        for p in c {
            x_min = x_min.min(p.x);
            y_min = y_min.min(p.y);
            x_max = x_max.max(p.x);
            y_max = y_max.max(p.y);
        }
    }

    let mut buf = Vec::with_capacity(64 + total_pts * 5);
    push_be16_i(&mut buf, g.contours.len() as i16);
    push_be16_i(&mut buf, x_min);
    push_be16_i(&mut buf, y_min);
    push_be16_i(&mut buf, x_max);
    push_be16_i(&mut buf, y_max);

    let mut acc = 0usize;
    for c in &g.contours {
        acc += c.len();
        push_be16(&mut buf, (acc - 1) as u16);
    }
    push_be16(&mut buf, 0); // instructionLength = 0

    for c in &g.contours {
        for p in c {
            buf.push(if p.on { 0x01 } else { 0x00 });
        }
    }

    let mut prev_x = 0i16;
    for c in &g.contours {
        for p in c {
            let dx = p.x - prev_x;
            prev_x = p.x;
            push_be16_i(&mut buf, dx);
        }
    }

    let mut prev_y = 0i16;
    for c in &g.contours {
        for p in c {
            let dy = p.y - prev_y;
            prev_y = p.y;
            push_be16_i(&mut buf, dy);
        }
    }

    pad4(&mut buf);
    buf
}

/// Compile the built-in vector `Sublime Sans` SFNT TrueType font byte stream for `TtfFont`.
pub fn build_sublime_sans_ttf() -> Vec<u8> {
    // Glyph 0 = .notdef, Glyphs 1..=95 = ASCII 32..=126
    let mut builders = Vec::with_capacity(96);
    builders.push(build_ascii_glyph(0));
    for ch in 32u8..=126u8 {
        builders.push(build_ascii_glyph(ch));
    }
    let num_glyphs = builders.len() as u16;

    // 1. glyf & loca
    let mut glyf = Vec::with_capacity(12 * 1024);
    let mut loca = Vec::with_capacity((num_glyphs as usize + 1) * 4);
    for b in &builders {
        push_be32(&mut loca, glyf.len() as u32);
        let enc = serialize_glyph(b);
        glyf.extend_from_slice(&enc);
    }
    push_be32(&mut loca, glyf.len() as u32);

    // 2. hmtx
    let mut hmtx = Vec::with_capacity(num_glyphs as usize * 4);
    for b in &builders {
        push_be16(&mut hmtx, b.advance);
        push_be16_i(&mut hmtx, b.lsb);
    }

    // 3. cmap (Format 4, single segment 32..=126 + 0xFFFF terminator)
    let mut cmap = Vec::new();
    push_be16(&mut cmap, 0); // version
    push_be16(&mut cmap, 1); // numTables
    push_be16(&mut cmap, 3); // platformID = Windows
    push_be16(&mut cmap, 1); // encodingID = Unicode BMP
    push_be32(&mut cmap, 12); // offset
    // Format 4 subtable: 2 segments (32..=126 -> delta = -31; 0xFFFF -> delta = +1)
    push_be16(&mut cmap, 4); // format
    push_be16(&mut cmap, 32); // length
    push_be16(&mut cmap, 0); // language
    push_be16(&mut cmap, 4); // segCountX2 (2 * 2)
    push_be16(&mut cmap, 4); // searchRange
    push_be16(&mut cmap, 1); // entrySelector
    push_be16(&mut cmap, 0); // rangeShift
    push_be16(&mut cmap, 126); // endCode[0]
    push_be16(&mut cmap, 0xFFFF); // endCode[1]
    push_be16(&mut cmap, 0); // reservedPad
    push_be16(&mut cmap, 32); // startCode[0]
    push_be16(&mut cmap, 0xFFFF); // startCode[1]
    push_be16_i(&mut cmap, -31); // idDelta[0]: 32 - 31 = 1
    push_be16_i(&mut cmap, 1); // idDelta[1]: 0xFFFF + 1 = 0
    push_be16(&mut cmap, 0); // idRangeOffset[0]
    push_be16(&mut cmap, 0); // idRangeOffset[1]

    // 4. head (54 bytes)
    let mut head = vec![0u8; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    head[4..8].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes()); // magicNumber
    head[16..18].copy_from_slice(&0x000Bu16.to_be_bytes());
    head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
    head[36..38].copy_from_slice(&0i16.to_be_bytes()); // xMin
    head[38..40].copy_from_slice(&(-220i16).to_be_bytes()); // yMin
    head[40..42].copy_from_slice(&820i16.to_be_bytes()); // xMax
    head[42..44].copy_from_slice(&780i16.to_be_bytes()); // yMax
    head[48..50].copy_from_slice(&8u16.to_be_bytes()); // lowestRecPPEM
    head[50..52].copy_from_slice(&1i16.to_be_bytes()); // indexToLocFormat = 1 (long u32)

    // 5. hhea (36 bytes)
    let mut hhea = vec![0u8; 36];
    hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    hhea[4..6].copy_from_slice(&820i16.to_be_bytes()); // ascent
    hhea[6..8].copy_from_slice(&(-220i16).to_be_bytes()); // descent
    hhea[8..10].copy_from_slice(&80i16.to_be_bytes()); // lineGap
    hhea[10..12].copy_from_slice(&840u16.to_be_bytes()); // advanceWidthMax
    hhea[34..36].copy_from_slice(&num_glyphs.to_be_bytes());

    // 6. maxp (6 bytes minimal v0.5/1.0 header needed by TtfFont)
    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&num_glyphs.to_be_bytes());

    // Assemble SFNT container in ascending tag order
    let mut tables: [(&[u8; 4], Vec<u8>); 7] = [
        (b"cmap", cmap),
        (b"glyf", glyf),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"loca", loca),
        (b"maxp", maxp),
    ];
    for (_, data) in &mut tables {
        pad4(data);
    }

    let num_tables = tables.len() as u16;
    let header_size = 12 + (num_tables as usize) * 16;
    let mut sfnt = Vec::with_capacity(header_size + 16384);
    push_be32(&mut sfnt, 0x0001_0000);
    push_be16(&mut sfnt, num_tables);
    push_be16(&mut sfnt, 64); // searchRange
    push_be16(&mut sfnt, 2);  // entrySelector
    push_be16(&mut sfnt, (num_tables * 16) - 64);

    let mut cur_off = header_size as u32;
    for (tag, data) in &tables {
        sfnt.extend_from_slice(*tag);
        push_be32(&mut sfnt, 0); // checksum
        push_be32(&mut sfnt, cur_off);
        push_be32(&mut sfnt, data.len() as u32);
        cur_off += data.len() as u32;
    }
    for (_, data) in &tables {
        sfnt.extend_from_slice(data);
    }
    sfnt
}
