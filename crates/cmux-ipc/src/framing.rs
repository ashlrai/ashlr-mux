pub fn split_lines(buffer: &str) -> Vec<&str> {
    buffer.lines().collect()
}

pub fn append_line(line: &str) -> String {
    let mut framed = String::with_capacity(line.len() + 1);
    framed.push_str(line);
    framed.push('\n');
    framed
}
