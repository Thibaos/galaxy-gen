use galaxy_gen::galaxy::GalaxyParams;
use galaxy_gen::render;

fn main() {
    let params = GalaxyParams::milky_way();

    render::render_top_down(&params, 2048, 360_000.0, 10, "galaxy.png");
}
