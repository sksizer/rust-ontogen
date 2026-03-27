use rust_template::Point;

fn main() {
    println!("Hello, world!");

    let point = Point::new(1, 2);

    // Convert the Point to a JSON string.
    let serialized = point.to_json();

    // Prints serialized = {"x":1,"y":2}
    println!("Serialized point = {serialized}");
}
