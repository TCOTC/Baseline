"""生成基线图标。

图标就是产品本身：一条向上的阶梯线。不做插画，不放字母缩写——
产品讲的就是「这条线在动」，图标不该讲别的东西。

用法：python scripts/icon.py

生成物提交进仓库（src-tauri/icons/），这样没有 Python 的环境照样能构建。
"""

from pathlib import Path

from PIL import Image, ImageDraw

# assets/view.css 里的 --blue。图标颜色和界面同源，不另起一套。
BLUE = (74, 111, 212, 255)

S = 1024
PAD = 148
STROKE = 104

# 阶梯线：先平着走一段，再抬两级台阶。
# 选 2 级而不是 3 级——32px 下 3 级会糊成一团，2 级还认得出是台阶。
POINTS = [
    (PAD, S - PAD),
    (392, S - PAD),
    (392, 620),
    (628, 620),
    (628, 396),
    (S - PAD, 396),
]


def render() -> Image.Image:
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # joint="curve" 处理拐角，端点的圆帽要自己补——PIL 的 line 不给端点加圆帽。
    d.line(POINTS, fill=BLUE, width=STROKE, joint="curve")
    r = STROKE // 2
    for x, y in (POINTS[0], POINTS[-1]):
        d.ellipse((x - r, y - r, x + r, y + r), fill=BLUE)
    return img


def main() -> None:
    img = render()
    out = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
    out.mkdir(parents=True, exist_ok=True)

    for name, size in {
        "32x32.png": 32,
        "128x128.png": 128,
        "128x128@2x.png": 256,
        "icon.png": 512,
    }.items():
        img.resize((size, size), Image.LANCZOS).save(out / name)

    # ico 要自带多个尺寸：任务栏、Alt-Tab、资源管理器各取各的，
    # 只塞一张大图的话小尺寸会被系统临时缩放出毛边。
    img.save(
        out / "icon.ico",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )

    for f in sorted(out.iterdir()):
        print(f"{f.name}  {f.stat().st_size} B")


if __name__ == "__main__":
    main()
