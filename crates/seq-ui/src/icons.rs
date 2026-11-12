//! Vector icons (20×20 grid from the mockup), drawn with the painter.

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Menu,
    Save,
    Play,
    Pause,
    Stop,
    Record,
    Metronome,
    Undo,
    Redo,
    Copy,
    Paste,
    Trash,
    Expand,
    Chevron,
    Pencil,
    Select,
    Eraser,
    Brush,
    Prev,
    Next,
    PlusCircle,
    Close,
}

/// Draws `icon` centered in `rect` (size `size` px), stroke width `w` (20-unit grid).
pub fn draw(p: &Painter, rect: Rect, size: f32, icon: Icon, color: Color32, w: f32) {
    let o = rect.center() - vec2(size / 2.0, size / 2.0);
    let k = size / 20.0;
    let pt = |x: f32, y: f32| -> Pos2 { pos2(o.x + x * k, o.y + y * k) };
    let s = Stroke::new(w * k, color);
    let line = |pts: &[(f32, f32)]| Shape::line(pts.iter().map(|&(x, y)| pt(x, y)).collect(), s);
    let closed = |pts: &[(f32, f32)]| Shape::closed_line(pts.iter().map(|&(x, y)| pt(x, y)).collect(), s);
    let fill = |pts: &[(f32, f32)]| Shape::convex_polygon(pts.iter().map(|&(x, y)| pt(x, y)).collect(), color, Stroke::NONE);
    let rrect = |x: f32, y: f32, ww: f32, hh: f32, r: f32| (Rect::from_min_size(pt(x, y), vec2(ww * k, hh * k)), r * k);
    let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| -> Vec<Pos2> {
        (0..=16).map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / 16.0;
            pt(cx + r * a.cos(), cy + r * a.sin())
        }).collect()
    };
    match icon {
        Icon::Menu => {
            for y in [5.0, 10.0, 15.0] {
                p.add(line(&[(3.0, y), (17.0, y)]));
            }
        }
        Icon::Save => {
            p.add(closed(&[(3.0, 3.0), (14.0, 3.0), (17.0, 6.0), (17.0, 17.0), (3.0, 17.0)]));
            p.add(line(&[(6.0, 3.0), (6.0, 8.0), (13.0, 8.0), (13.0, 3.0)]));
            p.add(line(&[(6.0, 17.0), (6.0, 12.0), (14.0, 12.0), (14.0, 17.0)]));
        }
        Icon::Play => {
            p.add(fill(&[(5.0, 3.0), (17.0, 10.0), (5.0, 17.0)]));
        }
        Icon::Pause => {
            let (a, r) = rrect(4.0, 3.0, 4.0, 14.0, 1.0);
            p.rect_filled(a, r, color);
            let (b, r) = rrect(12.0, 3.0, 4.0, 14.0, 1.0);
            p.rect_filled(b, r, color);
        }
        Icon::Stop => {
            let (a, r) = rrect(3.0, 3.0, 14.0, 14.0, 2.0);
            p.rect_filled(a, r, color);
        }
        Icon::Record => {
            p.circle_filled(pt(10.0, 10.0), 7.0 * k, color);
        }
        Icon::Metronome => {
            p.add(closed(&[(7.0, 2.0), (13.0, 2.0), (16.0, 18.0), (4.0, 18.0)]));
            p.add(line(&[(10.0, 13.0), (15.0, 5.0)]));
        }
        Icon::Undo => {
            p.add(line(&[(7.0, 4.0), (3.0, 8.0), (7.0, 12.0)]));
            let mut pts = vec![pt(3.0, 8.0), pt(12.0, 8.0)];
            pts.extend(arc(12.0, 13.0, 5.0, -std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2));
            pts.push(pt(9.0, 18.0));
            p.add(Shape::line(pts, s));
        }
        Icon::Redo => {
            p.add(line(&[(13.0, 4.0), (17.0, 8.0), (13.0, 12.0)]));
            let mut pts = vec![pt(17.0, 8.0), pt(8.0, 8.0)];
            pts.extend(arc(8.0, 13.0, 5.0, -std::f32::consts::FRAC_PI_2, -3.0 * std::f32::consts::FRAC_PI_2));
            pts.push(pt(11.0, 18.0));
            p.add(Shape::line(pts, s));
        }
        Icon::Copy => {
            let (a, r) = rrect(7.0, 7.0, 10.0, 10.0, 1.5);
            p.rect_stroke(a, r, s);
            p.add(line(&[(13.0, 4.0), (4.5, 4.0), (3.0, 5.5), (3.0, 13.0)]));
        }
        Icon::Paste => {
            let (a, r) = rrect(4.0, 4.0, 12.0, 14.0, 1.5);
            p.rect_stroke(a, r, s);
            let (b, r) = rrect(7.0, 2.0, 6.0, 4.0, 1.0);
            p.rect_stroke(b, r, s);
        }
        Icon::Trash => {
            p.add(line(&[(3.0, 5.0), (17.0, 5.0)]));
            p.add(line(&[(8.0, 5.0), (8.0, 3.0), (12.0, 3.0), (12.0, 5.0)]));
            p.add(line(&[(5.0, 5.0), (6.0, 17.0), (14.0, 17.0), (15.0, 5.0)]));
        }
        Icon::Expand => {
            p.add(line(&[(12.0, 3.0), (17.0, 3.0), (17.0, 8.0)]));
            p.add(line(&[(8.0, 17.0), (3.0, 17.0), (3.0, 12.0)]));
            p.add(line(&[(17.0, 3.0), (11.0, 9.0)]));
            p.add(line(&[(3.0, 17.0), (9.0, 11.0)]));
        }
        Icon::Chevron => {
            p.add(line(&[(5.0, 8.0), (10.0, 13.0), (15.0, 8.0)]));
        }
        Icon::Pencil => {
            p.add(closed(&[(3.0, 17.0), (4.0, 13.0), (13.0, 4.0), (16.0, 7.0), (7.0, 16.0)]));
        }
        Icon::Select => {
            let (a, _) = rrect(3.0, 3.0, 14.0, 14.0, 1.0);
            // Dashed rectangle.
            let corners = [a.left_top(), a.right_top(), a.right_bottom(), a.left_bottom(), a.left_top()];
            for seg in corners.windows(2) {
                p.extend(Shape::dashed_line(&[seg[0], seg[1]], s, 3.0 * k, 2.0 * k));
            }
        }
        Icon::Eraser => {
            p.add(line(&[(8.0, 17.0), (17.0, 17.0)]));
            p.add(closed(&[(3.0, 12.0), (10.0, 4.0), (16.0, 10.0), (10.0, 17.0), (8.0, 17.0)]));
        }
        Icon::Brush => {
            // Handle + tuft of bristles.
            p.add(line(&[(17.0, 3.0), (11.0, 9.0)]));
            p.add(closed(&[(9.0, 9.0), (11.5, 11.5), (8.5, 15.5), (3.0, 17.0), (4.5, 11.5)]));
        }
        Icon::Prev => {
            p.add(fill(&[(13.0, 3.0), (13.0, 17.0), (5.0, 10.0)]));
        }
        Icon::Next => {
            p.add(fill(&[(7.0, 3.0), (15.0, 10.0), (7.0, 17.0)]));
        }
        Icon::PlusCircle => {
            p.circle_stroke(pt(10.0, 10.0), 8.0 * k, s);
            p.add(line(&[(10.0, 6.0), (10.0, 14.0)]));
            p.add(line(&[(6.0, 10.0), (14.0, 10.0)]));
        }
        Icon::Close => {
            p.add(line(&[(5.0, 5.0), (15.0, 15.0)]));
            p.add(line(&[(15.0, 5.0), (5.0, 15.0)]));
        }
    }
}
