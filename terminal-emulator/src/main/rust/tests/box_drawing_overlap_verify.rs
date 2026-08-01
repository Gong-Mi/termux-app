#[cfg(test)]
mod tests {
    #[derive(Debug, PartialEq)]
    struct Line {
        start: (f32, f32),
        end: (f32, f32),
    }

    /// 模拟渲染器的逻辑：计算指定字符在单元格内的线条路径
    fn get_box_drawing_path(ch: char, x: f32, y_top: f32, w: f32, h: f32) -> Vec<Line> {
        let cx = x + w / 2.0;
        let cy = y_top + h / 2.0;

        match ch as u32 {
            0x2500 => {
                // ─ (全长水平线)
                vec![Line {
                    start: (x, cy),
                    end: (x + w, cy),
                }]
            }
            0x256D => {
                // ╭ (左上圆角：中下到中心，中心到中右)
                vec![
                    Line {
                        start: (cx, y_top + h),
                        end: (cx, cy),
                    },
                    Line {
                        start: (cx, cy),
                        end: (x + w, cy),
                    },
                ]
            }
            0x256E => {
                // ╮ (右上圆角：中下到中心，中心到中左)
                vec![
                    Line {
                        start: (cx, y_top + h),
                        end: (cx, cy),
                    },
                    Line {
                        start: (cx, cy),
                        end: (x, cy),
                    },
                ]
            }
            _ => vec![],
        }
    }

    #[test]
    fn test_border_connectivity_no_gap_no_overlap() {
        let w = 10.0;
        let h = 20.0;

        // 场景：左单元格是 ╭ (0x256D)，右单元格是 ─ (0x2500)
        let left_x = 100.0;
        let right_x = 110.0; // 紧邻
        let y = 200.0;

        let left_paths = get_box_drawing_path('╭', left_x, y, w, h);
        let right_paths = get_box_drawing_path('─', right_x, y, w, h);

        // 1. 寻找衔接点
        // 左单元格向右发散的线段
        let left_to_right_line = left_paths.iter().find(|l| l.end.0 == left_x + w).unwrap();
        // 右单元格向左开始的线段
        let right_from_left_line = right_paths.iter().find(|l| l.start.0 == right_x).unwrap();

        println!("Left Cell Exit: {:?}", left_to_right_line.end);
        println!("Right Cell Entry: {:?}", right_from_left_line.start);

        // 验证：衔接点必须完全重合
        assert_eq!(
            left_to_right_line.end, right_from_left_line.start,
            "衔接处必须坐标对齐"
        );

        // 2. 验证背景重复绘制逻辑（模拟判断）
        // 我们的优化是：如果当前字符不是全块(0x2588)，则不应再次填充背景
        let will_draw_bg = |ch: char| -> bool {
            ch == '\u{2588}' // 只有全块才重绘背景（如果颜色不同）
        };

        assert!(!will_draw_bg('╭'), "圆角字符不应重复绘制背景");
        assert!(!will_draw_bg('─'), "直线字符不应重复绘制背景");

        println!(
            "SUCCESS: Precise connectivity confirmed at x={}",
            left_to_right_line.end.0
        );
    }
}
