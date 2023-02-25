import subprocess
import tempfile

from pathlib import Path
from xml.dom import minidom

black_piece_template = '''
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <defs
     id="defs961">
    <filter
       style="color-interpolation-filters:sRGB;"
       id="filter3216">
      <feGaussianBlur
         in="SourceGraphic"
         stdDeviation="1.6"
         result="blur"
         id="feGaussianBlur3206" />
      <feDiffuseLighting
         diffuseConstant="1"
         surfaceScale="10"
         lighting-color="rgb(255,255,255)"
         result="diffuse"
         id="feDiffuseLighting3210">
        <feDistantLight
           elevation="150"
           azimuth="0"
           id="feDistantLight3208" />
      </feDiffuseLighting>
      <feComposite
         in="diffuse"
         in2="diffuse"
         operator="arithmetic"
         k1="1"
         result="composite1"
         id="feComposite3212" />
      <feComposite
         in="composite1"
         in2="SourceGraphic"
         k1="0.909804"
         operator="arithmetic"
         k3="1"
         result="fbSourceGraphic"
         id="feComposite3214" />
      <feColorMatrix
         result="fbSourceGraphicAlpha"
         in="fbSourceGraphic"
         values="0 0 0 -1 0 0 0 0 -1 0 0 0 0 -1 0 0 0 0 1 0"
         id="feColorMatrix3218" />
      <feGaussianBlur
         id="feGaussianBlur3220"
         in="fbSourceGraphic"
         stdDeviation="1.6"
         result="blur" />
      <feDiffuseLighting
         id="feDiffuseLighting3222"
         diffuseConstant="1"
         surfaceScale="10"
         lighting-color="rgb(255,255,255)"
         result="diffuse">
        <feDistantLight
           id="feDistantLight3224"
           elevation="150"
           azimuth="0" />
      </feDiffuseLighting>
      <feComposite
         in2="diffuse"
         id="feComposite3226"
         in="diffuse"
         operator="arithmetic"
         k1="1"
         result="composite1" />
      <feComposite
         in2="fbSourceGraphic"
         id="feComposite3228"
         in="composite1"
         k1="0.909804"
         operator="arithmetic"
         k3="1"
         result="composite2" />
    </filter>
  </defs>
  <path
     id="path"
     style="display:inline;fill-rule:evenodd;fill:#151515;fill-opacity:1;stroke:#000000;stroke-opacity:1;stroke-width:1.5;stroke-linejoin:round;stroke-linecap:round;filter:url(#filter3216)"
     d="{path}" />
</svg>
'''

white_piece_template = '''
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <defs
     id="defs961">
    <filter
       style="color-interpolation-filters:sRGB;"
       id="filter3216">
      <feGaussianBlur
         in="SourceGraphic"
         stdDeviation="1.6"
         result="blur"
         id="feGaussianBlur3206" />
      <feDiffuseLighting
         diffuseConstant="1"
         surfaceScale="10"
         lighting-color="rgb(255,255,255)"
         result="diffuse"
         id="feDiffuseLighting3210">
        <feDistantLight
           elevation="150"
           azimuth="0"
           id="feDistantLight3208" />
      </feDiffuseLighting>
      <feComposite
         in="diffuse"
         in2="diffuse"
         operator="arithmetic"
         k1="1"
         result="composite1"
         id="feComposite3212" />
      <feComposite
         in="composite1"
         in2="SourceGraphic"
         k1="0.909804"
         operator="arithmetic"
         k3="1"
         result="fbSourceGraphic"
         id="feComposite3214" />
      <feColorMatrix
         result="fbSourceGraphicAlpha"
         in="fbSourceGraphic"
         values="0 0 0 -1 0 0 0 0 -1 0 0 0 0 -1 0 0 0 0 1 0"
         id="feColorMatrix3218" />
      <feGaussianBlur
         id="feGaussianBlur3220"
         in="fbSourceGraphic"
         stdDeviation="1.6"
         result="blur" />
      <feDiffuseLighting
         id="feDiffuseLighting3222"
         diffuseConstant="1"
         surfaceScale="10"
         lighting-color="rgb(255,255,255)"
         result="diffuse">
        <feDistantLight
           id="feDistantLight3224"
           elevation="150"
           azimuth="0" />
      </feDiffuseLighting>
      <feComposite
         in2="diffuse"
         id="feComposite3226"
         in="diffuse"
         operator="arithmetic"
         k1="1"
         result="composite1" />
      <feComposite
         in2="fbSourceGraphic"
         id="feComposite3228"
         in="composite1"
         k1="0.909804"
         operator="arithmetic"
         k3="1"
         result="composite2" />
    </filter>
    <filter
       style="color-interpolation-filters:sRGB;"
       id="filter3240">
      <feGaussianBlur
         in="SourceGraphic"
         stdDeviation="1.6"
         result="blur"
         id="feGaussianBlur3230" />
      <feDiffuseLighting
         diffuseConstant="1"
         surfaceScale="10"
         lighting-color="rgb(255,255,255)"
         result="diffuse"
         id="feDiffuseLighting3234">
        <feDistantLight
           elevation="150"
           azimuth="0"
           id="feDistantLight3232" />
      </feDiffuseLighting>
      <feComposite
         in="diffuse"
         in2="diffuse"
         operator="arithmetic"
         k1="1"
         result="composite1"
         id="feComposite3236" />
      <feComposite
         in="composite1"
         in2="SourceGraphic"
         k1="0.8"
         operator="arithmetic"
         k3="1"
         result="composite2"
         id="feComposite3238" />
    </filter>
  </defs>
  <path
     id="path"
     style="display:inline;fill-rule:evenodd;fill:#cfcfcf;fill-opacity:1;stroke:#000000;stroke-opacity:1;stroke-width:1.5;stroke-linejoin:round;stroke-linecap:round;filter:url(#filter3240)"
     d="{path}" />
</svg>
'''

# INKSCAPE_PATH = 'inkscape'
INKSCAPE_PATH = r'C:\Program Files\Inkscape\bin\inkscape.exe'
TEMP_PATH = Path('rendering-tmp.svg')

for input in Path('pieces-svg').glob('*.svg'):
    doc = minidom.parse(input.as_posix())
    path_strings = [p.getAttribute('d') for p in doc.getElementsByTagName('path')]
    assert(len(path_strings) == 1)
    doc.unlink()

    for is_white in [False, True]:
        out_prefix = 'white' if is_white else 'black'
        out = Path('../assets/pieces') / f'{out_prefix}-{input.stem}.png'
        template = white_piece_template if is_white else black_piece_template
        print(f'{input} -> {out}')
        TEMP_PATH.write_text(template.format(path = path_strings[0]))
        subprocess.run(
            [
                INKSCAPE_PATH,
                '--export-width=320',
                '--export-type=png',
                f'--export-filename={out}',
                TEMP_PATH
            ],
            check=True
        )
        TEMP_PATH.unlink()
