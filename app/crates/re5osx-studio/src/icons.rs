//! Inline SVG icons (stroke = currentColor).

macro_rules! icon {
    ($name:ident, $size:literal, $body:literal) => {
        pub const $name: &str = concat!(
            "<svg width=\"", $size, "\" height=\"", $size,
            "\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.8\" stroke-linecap=\"round\" stroke-linejoin=\"round\">",
            $body, "</svg>"
        );
    };
}

icon!(FOLDER, "21", "<path d=\"M4 5.5A1.5 1.5 0 0 1 5.5 4h4l2 2.5h7A1.5 1.5 0 0 1 20 8v10.5a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 18.5z\"/>");
icon!(BOOK, "21", "<path d=\"M5 4.5h9.5L19 9v10.5H5z\"/><path d=\"M14 4.5V9h5M8.5 13h7M8.5 16.5h5\"/>");
icon!(WARNING, "21", "<path d=\"M12 4 3 19.5h18z\"/><path d=\"M12 10v4.5M12 17v.01\"/>");
icon!(ERROR, "21", "<circle cx=\"12\" cy=\"12\" r=\"9\"/><path d=\"M12 7.5v5M12 16v.01\"/>");
icon!(TERMINAL, "21", "<rect x=\"3.5\" y=\"5\" width=\"17\" height=\"14\" rx=\"2\"/><path d=\"m7.5 10 3 2.5-3 2.5M12.5 15.5h4\"/>");
icon!(SLIDERS, "21", "<path d=\"M4 7h9M17 7h3M4 17h3M11 17h9\"/><circle cx=\"15\" cy=\"7\" r=\"2\"/><circle cx=\"9\" cy=\"17\" r=\"2\"/>");
icon!(CODE, "21", "<path d=\"m8 8-4 4 4 4M16 8l4 4-4 4M13.5 5l-3 14\"/>");
icon!(SEARCH, "15", "<circle cx=\"11\" cy=\"11\" r=\"7\"/><path d=\"m20 20-3.5-3.5\"/>");
icon!(IMPORT, "15", "<path d=\"M12 4v11M7.5 10.5 12 15l4.5-4.5M5 19h14\"/>");
icon!(PENCIL, "13", "<path d=\"M4 20h4L19 9l-4-4L4 16z\"/><path d=\"m13.5 6.5 4 4\"/>");
icon!(TRASH, "13", "<path d=\"M5 7h14M10 7V4.5h4V7M7 7l1 13h8l1-13\"/>");
icon!(PLUS, "15", "<path d=\"M12 5v14M5 12h14\"/>");
icon!(FILE, "15", "<path d=\"M6 3.5h8l4 4v13H6z\"/><path d=\"M14 3.5v4h4\"/>");
icon!(LOCK, "11", "<rect x=\"5\" y=\"10.5\" width=\"14\" height=\"10\" rx=\"2\"/><path d=\"M8.5 10.5V7.5a3.5 3.5 0 0 1 7 0v3\"/>");
icon!(RESTART, "13", "<path d=\"M19 12a7 7 0 1 1-2.1-5M19 4.5V8h-3.5\"/>");
pub const STOP: &str = "<svg width=\"12\" height=\"12\" viewBox=\"0 0 24 24\" fill=\"currentColor\"><rect x=\"5\" y=\"5\" width=\"14\" height=\"14\" rx=\"2\"/></svg>";
pub const PLAY: &str = "<svg width=\"12\" height=\"12\" viewBox=\"0 0 24 24\" fill=\"currentColor\"><path d=\"M7 4.5v15l13-7.5z\"/></svg>";
