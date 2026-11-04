"""重新生成 OCR 标定基准集（可选：仓库里已带好 png，跑基准不需要 Python）。

    python tools/ocr-bench/generate.py

覆盖的失败模式（每张都配同名 .txt 参考答案）：
- 深色 UI 白字 / 浅色底黑字；
- 16 / 24 / 14 / 18 / 28 px 五种字号（屏幕截图里最常见的一档到三档）；
- 中文与英文（中文考分词间隙，英文考词间空格）；
- 多行（考框会不会串行 / 阅读顺序）。
"""

import os

from PIL import Image, ImageDraw, ImageFont

OUT = os.path.dirname(os.path.abspath(__file__))
FONT = "C:/Windows/Fonts/msyh.ttc"
DARK_BG = (32, 32, 36)
DARK_FG = (228, 230, 235)

SINGLE = [
    ("dark-16", DARK_BG, DARK_FG, 16,
     "If you cannot explain it simply, you do not understand it well enough."),
    ("dark-24", DARK_BG, DARK_FG, 24, "A lemma is the canonical form of a word."),
    ("light-14", (255, 255, 255), (24, 24, 28), 14,
     "Deep reading means following an idea down the rabbit hole."),
    ("dark-zh-18", (30, 30, 34), (222, 224, 230), 18, "深度阅读需要把原文和译文放在一起对照。"),
    ("dark-zh-28", (30, 30, 34), (222, 224, 230), 28, "阈值很重要，但这个约束更加微妙。"),
]

MULTI_LINES = [
    "Reading flow matters more than lookup speed.",
    "A lemma is the canonical form of a word.",
    "It keeps every inflection pointing back to it.",
]


def save(name: str, image: Image.Image, expected: str) -> None:
    image.save(os.path.join(OUT, name + ".png"))
    with open(os.path.join(OUT, name + ".txt"), "w", encoding="utf-8") as handle:
        handle.write(expected)
    print(name, image.size)


def main() -> None:
    for name, bg, fg, size, text in SINGLE:
        font = ImageFont.truetype(FONT, size)
        pad = 20
        probe = Image.new("RGB", (10, 10))
        box = ImageDraw.Draw(probe).textbbox((0, 0), text, font=font)
        image = Image.new(
            "RGB", (box[2] - box[0] + pad * 2, box[3] - box[1] + pad * 2), bg
        )
        ImageDraw.Draw(image).text(
            (pad - box[0], pad - box[1]), text, font=font, fill=fg
        )
        save(name, image, text)

    # 多行：正常行距，最容易暴露"框太大把邻行吃进来"
    for name, bg, fg, size, spacing in [
        ("multi-dark-18", DARK_BG, DARK_FG, 18, 8),
        ("multi-dark-24", (30, 30, 34), (222, 224, 230), 24, 10),
    ]:
        font = ImageFont.truetype(FONT, size)
        pad = 16
        probe = ImageDraw.Draw(Image.new("RGB", (10, 10)))
        width = max(probe.textbbox((0, 0), line, font=font)[2] for line in MULTI_LINES)
        line_height = size + spacing
        image = Image.new(
            "RGB", (width + pad * 2, line_height * len(MULTI_LINES) + pad * 2), bg
        )
        draw = ImageDraw.Draw(image)
        for index, line in enumerate(MULTI_LINES):
            draw.text((pad, pad + index * line_height), line, font=font, fill=fg)
        save(name, image, "\n".join(MULTI_LINES))


if __name__ == "__main__":
    main()
