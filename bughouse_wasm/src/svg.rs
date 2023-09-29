use std::f64::consts::PI;

pub fn polar_to_cartesian(center_x: f64, center_y: f64, radius: f64, rad: f64) -> (f64, f64) {
    (center_x + (radius * rad.cos()), center_y + (radius * rad.sin()))
}

pub fn ring_arc_path(
    x: f64, y: f64, inner_radius: f64, outer_radius: f64, start_rad: f64, end_rad: f64,
) -> String {
    let (inner_start_x, inner_start_y) = polar_to_cartesian(x, y, inner_radius, end_rad);
    let (inner_end_x, inner_end_y) = polar_to_cartesian(x, y, inner_radius, start_rad);
    let (outer_start_x, outer_start_y) = polar_to_cartesian(x, y, outer_radius, end_rad);
    let (outer_end_x, outer_end_y) = polar_to_cartesian(x, y, outer_radius, start_rad);

    let large_arc_flag = if end_rad - start_rad <= PI { "0" } else { "1" };

    format!(
        "M {inner_start_x} {inner_start_y} L {outer_start_x} {outer_start_y} \
        A {outer_radius} {outer_radius} 0 {large_arc_flag} 0 {outer_end_x} {outer_end_y} \
        L {inner_end_x} {inner_end_y} \
        A {inner_radius} {inner_radius} 0 {large_arc_flag} 1 {inner_start_x} {inner_start_y}"
    )
}
