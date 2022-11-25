use eframe::epaint::Color32;

pub struct ColourProxy(Color32);

impl ColourProxy {
    pub fn via_rgb<F: FnMut(rgb::RGB<u8>) -> rgb::RGB<u8>>(&self, mut f: F) -> Self {
        let rgb = self.into();
        let rgb = f(rgb);
        Self(Color32::from_rgb(rgb.r, rgb.g, rgb.b))
    }

    pub fn via_rgba<F: FnMut(rgb::RGBA<u8>) -> rgb::RGBA<u8>>(&self, mut f: F) -> Self {
        let rgba = self.into();
        let rgba = f(rgba);
        Self(Color32::from_rgba_premultiplied(
            rgba.r, rgba.g, rgba.b, rgba.a,
        ))
    }
}

impl From<&Color32> for ColourProxy {
    fn from(colour: &Color32) -> Self {
        Self(*colour)
    }
}

impl From<Color32> for ColourProxy {
    fn from(colour: Color32) -> Self {
        Self(colour)
    }
}

impl From<&ColourProxy> for Color32 {
    fn from(proxy: &ColourProxy) -> Self {
        proxy.0
    }
}

impl From<ColourProxy> for Color32 {
    fn from(proxy: ColourProxy) -> Self {
        proxy.0
    }
}

impl From<&ColourProxy> for rgb::RGB<u8> {
    fn from(proxy: &ColourProxy) -> Self {
        let ColourProxy(colour) = proxy;
        rgb::RGB::new(colour.r(), colour.g(), colour.b())
    }
}

impl From<ColourProxy> for rgb::RGB<u8> {
    fn from(proxy: ColourProxy) -> Self {
        let ColourProxy(colour) = proxy;
        rgb::RGB::new(colour.r(), colour.g(), colour.b())
    }
}

impl From<&rgb::RGB<u8>> for ColourProxy {
    fn from(rgb: &rgb::RGB<u8>) -> Self {
        Self(Color32::from_rgb(rgb.r, rgb.g, rgb.b))
    }
}

impl From<rgb::RGB<u8>> for ColourProxy {
    fn from(rgb: rgb::RGB<u8>) -> Self {
        Self(Color32::from_rgb(rgb.r, rgb.g, rgb.b))
    }
}

impl From<&rgb::RGBA<u8>> for ColourProxy {
    fn from(rgba: &rgb::RGBA<u8>) -> Self {
        Self(Color32::from_rgba_premultiplied(
            rgba.r, rgba.g, rgba.b, rgba.a,
        ))
    }
}

impl From<ColourProxy> for rgb::RGBA<u8> {
    fn from(proxy: ColourProxy) -> Self {
        let ColourProxy(colour) = proxy;
        rgb::RGBA::new(colour.r(), colour.g(), colour.b(), colour.a())
    }
}

impl From<rgb::RGBA<u8>> for ColourProxy {
    fn from(rgba: rgb::RGBA<u8>) -> Self {
        Self(Color32::from_rgba_premultiplied(
            rgba.r, rgba.g, rgba.b, rgba.a,
        ))
    }
}

impl From<&ColourProxy> for rgb::RGBA<u8> {
    fn from(proxy: &ColourProxy) -> Self {
        let ColourProxy(colour) = proxy;
        rgb::RGBA::new(colour.r(), colour.g(), colour.b(), colour.a())
    }
}

pub trait ExtendedColourOps {
    fn via_rgb<F: FnMut(rgb::RGB<u8>) -> rgb::RGB<u8>>(&self, f: F) -> Self;
    fn via_rgba<F: FnMut(rgb::RGBA<u8>) -> rgb::RGBA<u8>>(&self, f: F) -> Self;
    fn map_rgb<F: FnMut(u8) -> u8>(&self, f: F) -> Self;
    fn map_rgba<F: FnMut(u8) -> u8>(&self, f: F) -> Self;
    fn mix(&self, ratio: f32, other: &Self) -> Self;
}

impl ExtendedColourOps for Color32 {
    fn via_rgb<F: FnMut(rgb::RGB<u8>) -> rgb::RGB<u8>>(&self, f: F) -> Self {
        let proxy: ColourProxy = self.into();
        let rgb = proxy.via_rgb(f);
        rgb.into()
    }

    fn via_rgba<F: FnMut(rgb::RGBA<u8>) -> rgb::RGBA<u8>>(&self, f: F) -> Self {
        let proxy: ColourProxy = self.into();
        let rgb = proxy.via_rgba(f);
        rgb.into()
    }

    fn map_rgb<F: FnMut(u8) -> u8>(&self, mut f: F) -> Self {
        use rgb::*;
        self.via_rgb(|rgb| rgb.map(&mut f))
    }

    fn map_rgba<F: FnMut(u8) -> u8>(&self, mut f: F) -> Self {
        use rgb::*;
        self.via_rgba(|rgba| rgba.map(&mut f))
    }

    fn mix(&self, ratio: f32, other: &Self) -> Self {
        Color32::from_rgb(
            ((1.0 - ratio) * self.r() as f32 + ratio * other.r() as f32) as u8,
            ((1.0 - ratio) * self.g() as f32 + ratio * other.g() as f32) as u8,
            ((1.0 - ratio) * self.b() as f32 + ratio * other.b() as f32) as u8,
        )
    }
}
