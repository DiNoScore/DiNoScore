# Getting sheets prepared for DiNoScore

## Already imported sheets

Some day, there will be a public repository with a few songs in it. TODO

## Get them on the internet

If you're lucky, you'll find a neat PDF of the song you're looking for. If you're playing songs from dead composers, definitely have a seach in [IMSLP](http://imslp.org/). It will cover you with high-quality processed scans of the paper sheets.

## Please help, I have got some paper!

You have got two options: either scan them or take a photo. Scanning is slow and tedious (except if you've got a really expensive machine), but it will generally give better results and require less post-processing. Music books tent to be *slightly* too large to fit onto the scanning area (and that's probably not by accident).

When taking photographs of the sheet, try to keep the sheet flat and the lighting consistent. Take the photo with maximum resolution, there will be some losses. If possible, take the photo from far away with optical zoom; this reduces perspective distortions. Take one picture per page.

To keep the book flat (some are really sturdy), you can press it against some window and then photograph it from the other side.

### Post-processing in GIMP

Open the image in GIMP. Do less work and skip steps if the image looks good enough. If the scan is really good quality, you can skip it alltogether and let the editor do the remaining work for you.

1. **Perspective distortion:** If the image is distorted, [use the perspective tool](https://graphicdesign.stackexchange.com/a/102032) to get it straight. This is tedious, only use on heavy distortion. The tool will give you a floating selection. This tool best works if the image has generous margins around the border of the page.
    - When doing step 1. you can probably skip step 2. and replace step 3. with a simple "Crop to content".
2. **Rotation:** Use the "arbitrary rotation" tool to get upside up. It probably won't perfect due to some remaining distortions, but that's fine.
3. **Cropping:** Now, crop the image to the page's bounds.
4. **Color correction 1:** The "color to gray" tool is a real gift for this task and does a lot of the work. But a simple desaturate works as well.
5. **Color correction 2:** You need to do color balance and binarisation. I recommend the "levels" tool for this.
    - Drag the white handle down over the first large peak (or until the page is totally white).
    - Drag the black handle up until all the ink is pure black, but stop before the sheet is getting darker.
    - Adjust the gray handle so that all staff lines are legible.
    - A strong black/white contrast is good enough, you don't need to perform actual binarization.
6. **Color correction 3:** The previous steps might have given you some transparent pixels. Make them white using "Layer → Transparency → Remove Alpha Channel".
7. **Scale down:** You probably don't need your sheets to be in 4k. With some generous JPEG compression (80% or so), you'll get sufficiently small file sizes. Calculate less than 1 MiB per page.

## Import the scans using the editor

The editor works with all kinds of images, as well as PDF.

1. If your image already is black and white, add it using TODO.
2. If your image is in colour, you can use TODO. This will perform a simpler binarization for you. It works well in mild cases, but if it doesn't you'll have to use GIMP.
3. Select all pages that contain staves, and click "Autodetect". This will take a while, and eat your RAM. If you're unsure, start with trying out only the first page.
4. If the automatic detection did something wrong, you're out of luck. :D There are no manual editing features as of now.
5. Select the staves and annotate them appropriately:
    - For each "song" in the sheets, click "part start" and give it a name.
    - Use "start section" to divide the song further. Create at least one section for each repetition.
    - Click "is repetition" on the first section of every repetition
    - If the repetition does not end at the end of a staff:
        - Mark that staff as the start of a new sections
        - Click "previous section ends here"
6. Save
