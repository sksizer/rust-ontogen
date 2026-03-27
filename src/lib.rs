use serde::Serialize;

#[derive(Serialize, Debug)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Failed to serialize Point")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_point() {
        let point = Point::new(1, 2);
        assert_eq!(point.x, 1);
        assert_eq!(point.y, 2);
    }

    #[test]
    fn test_to_json() {
        let point = Point::new(1, 2);
        assert_eq!(point.to_json(), r#"{"x":1,"y":2}"#);
    }
}
