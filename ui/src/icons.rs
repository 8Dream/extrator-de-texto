//! Icones simples desenhados com tracos (sem imagens nem fontes de icone).

use eframe::egui::{pos2, Color32, Painter, Rect, Shape, Stroke, StrokeKind};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icone {
    Converter,
    Lote,
    Microfone,
    Execucoes,
    Log,
    Ajustes,
    Ajuda,
    Lua,
    Sol,
    Enviar,
    Paleta,
    Casa,
}

/// Desenha o icone dentro de `r` (grade de 24x24, como os icones da interface HTML).
pub fn desenhar(p: &Painter, r: Rect, ic: Icone, cor: Color32) {
    let k = r.width().min(r.height()) / 24.0;
    let o = r.center() - eframe::egui::vec2(12.0 * k, 12.0 * k);
    let pt = |x: f32, y: f32| pos2(o.x + x * k, o.y + y * k);
    let tr = Stroke::new(1.8 * k.max(0.8), cor);
    let linha = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([pt(a.0, a.1), pt(b.0, b.1)], tr);
    };
    let poli = |pts: &[(f32, f32)], fechar: bool| {
        let v: Vec<_> = pts.iter().map(|(x, y)| pt(*x, *y)).collect();
        p.add(if fechar {
            Shape::closed_line(v, tr)
        } else {
            Shape::line(v, tr)
        });
    };
    let caixa = |x0: f32, y0: f32, x1: f32, y1: f32, raio: f32| {
        p.rect_stroke(
            Rect::from_min_max(pt(x0, y0), pt(x1, y1)),
            raio * k,
            tr,
            StrokeKind::Middle,
        );
    };
    match ic {
        Icone::Converter => {
            poli(
                &[
                    (14.0, 3.0),
                    (7.0, 3.0),
                    (5.0, 5.0),
                    (5.0, 19.0),
                    (7.0, 21.0),
                    (17.0, 21.0),
                    (19.0, 19.0),
                    (19.0, 8.0),
                ],
                false,
            );
            poli(&[(14.0, 3.0), (19.0, 8.0), (14.0, 8.0)], true);
            linha((9.0, 13.0), (15.0, 13.0));
            linha((9.0, 17.0), (13.0, 17.0));
        }
        Icone::Lote => {
            poli(
                &[
                    (3.0, 7.0),
                    (5.0, 5.0),
                    (9.0, 5.0),
                    (11.0, 7.0),
                    (19.0, 7.0),
                    (21.0, 9.0),
                    (21.0, 17.0),
                    (19.0, 19.0),
                    (5.0, 19.0),
                    (3.0, 17.0),
                ],
                true,
            );
        }
        Icone::Microfone => {
            caixa(9.0, 3.0, 15.0, 14.0, 3.0);
            poli(
                &[
                    (5.0, 11.0),
                    (6.0, 14.5),
                    (9.0, 17.5),
                    (12.0, 18.5),
                    (15.0, 17.5),
                    (18.0, 14.5),
                    (19.0, 11.0),
                ],
                false,
            );
            linha((12.0, 18.5), (12.0, 21.0));
        }
        Icone::Execucoes => {
            linha((4.0, 6.0), (20.0, 6.0));
            linha((4.0, 12.0), (14.0, 12.0));
            linha((4.0, 18.0), (10.0, 18.0));
            poli(&[(16.0, 15.0), (18.0, 17.0), (22.0, 13.0)], false);
        }
        Icone::Log => {
            caixa(4.0, 3.0, 20.0, 21.0, 2.0);
            linha((8.0, 8.0), (16.0, 8.0));
            linha((8.0, 12.0), (16.0, 12.0));
            linha((8.0, 16.0), (13.0, 16.0));
        }
        Icone::Ajustes => {
            linha((4.0, 6.0), (14.0, 6.0));
            linha((18.0, 6.0), (20.0, 6.0));
            linha((4.0, 12.0), (8.0, 12.0));
            linha((12.0, 12.0), (20.0, 12.0));
            linha((4.0, 18.0), (16.0, 18.0));
            p.circle_stroke(pt(16.0, 6.0), 2.0 * k, tr);
            p.circle_stroke(pt(10.0, 12.0), 2.0 * k, tr);
            p.circle_stroke(pt(18.0, 18.0), 2.0 * k, tr);
        }
        Icone::Ajuda => {
            p.circle_stroke(pt(12.0, 12.0), 9.0 * k, tr);
            poli(
                &[
                    (9.5, 9.5),
                    (10.5, 8.0),
                    (12.5, 7.6),
                    (14.3, 8.6),
                    (14.5, 10.4),
                    (13.0, 11.8),
                    (12.0, 13.2),
                ],
                false,
            );
            p.circle_filled(pt(12.0, 17.0), 1.1 * k, cor);
        }
        Icone::Lua => {
            // Contorno de lua crescente: arco externo (centro 12,12 raio 9) e arco interno (centro 16,8 raio 7).
            let mut pts: Vec<(f32, f32)> = Vec::new();
            let mut a = 5.0_f32;
            while a <= 265.0 {
                let r = a.to_radians();
                pts.push((12.0 + 9.0 * r.cos(), 12.0 + 9.0 * r.sin()));
                a += 12.0;
            }
            let mut a = 217.0_f32;
            while a >= 53.0 {
                let r = a.to_radians();
                pts.push((16.8 + 7.0 * r.cos(), 7.2 + 7.0 * r.sin()));
                a -= 12.0;
            }
            poli(&pts, true);
        }
        Icone::Sol => {
            p.circle_stroke(pt(12.0, 12.0), 4.5 * k, tr);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (s, c) = a.sin_cos();
                p.line_segment(
                    [
                        pt(12.0 + 7.5 * c, 12.0 + 7.5 * s),
                        pt(12.0 + 10.0 * c, 12.0 + 10.0 * s),
                    ],
                    tr,
                );
            }
        }
        Icone::Paleta => {
            // Paleta de pintor: contorno com um recorte e quatro manchas de tinta.
            let mut pts: Vec<(f32, f32)> = Vec::new();
            let mut a = 40.0_f32;
            while a <= 400.0 {
                let r = a.to_radians();
                pts.push((12.0 + 9.5 * r.cos(), 12.0 + 9.0 * r.sin()));
                a += 10.0;
            }
            poli(&pts, false);
            poli(
                &[(19.5, 16.0), (16.0, 16.0), (14.5, 18.5), (16.5, 21.0)],
                false,
            );
            for (x, y) in [(7.5, 11.0), (10.5, 7.5), (15.0, 7.5), (18.0, 11.0)] {
                p.circle_filled(pt(x, y), 1.5 * k, cor);
            }
        }
        Icone::Casa => {
            poli(&[(3.0, 11.0), (12.0, 3.0), (21.0, 11.0)], false);
            poli(&[(5.5, 9.5), (5.5, 20.0), (18.5, 20.0), (18.5, 9.5)], false);
            poli(
                &[(10.0, 20.0), (10.0, 14.0), (14.0, 14.0), (14.0, 20.0)],
                false,
            );
        }
        Icone::Enviar => {
            linha((12.0, 16.0), (12.0, 4.0));
            poli(&[(8.0, 8.0), (12.0, 4.0), (16.0, 8.0)], false);
            poli(
                &[
                    (4.0, 16.0),
                    (4.0, 19.0),
                    (5.0, 20.0),
                    (19.0, 20.0),
                    (20.0, 19.0),
                    (20.0, 16.0),
                ],
                false,
            );
        }
    }
}
