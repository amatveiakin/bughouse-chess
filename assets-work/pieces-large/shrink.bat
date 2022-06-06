@REM Note. Gimp gives slightly better file sizes in my experiments.
@REM Improvement potential: Play with ffmpeg options or re-shrink using Gimp.

for %%f in (*.png) do ffmpeg -i %%f -vf scale=320:-1 ..\..\assets\pieces\%%f
