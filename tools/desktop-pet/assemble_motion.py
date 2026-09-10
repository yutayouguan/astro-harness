"""Assemble already-matted AI pose grids; preserves one scale and ground anchor.

No image API calls or generated in-betweens. Reordering is explicit in the report.
Requires Pillow. Output is an Astro motion clip plus a contact sheet and GIF.
"""
import argparse
import json
from collections import deque
from pathlib import Path
from PIL import Image, ImageDraw


def pose_bounds(image):
    """Recover whole silhouettes when generated poses drift across guide cells."""
    w,h=image.size
    mask=bytearray(1 if value>32 else 0 for value in image.getchannel("A").getdata())
    found=[]
    for start in range(len(mask)):
        if not mask[start]: continue
        mask[start]=0; queue=deque([start]); count=0
        left=right=start%w; top=bottom=start//w
        while queue:
            i=queue.popleft(); x=i%w; y=i//w; count+=1
            left=min(left,x); right=max(right,x); top=min(top,y); bottom=max(bottom,y)
            for j in ((i-1 if x else -1),(i+1 if x+1<w else -1),i-w,i+w):
                if 0<=j<len(mask) and mask[j]: mask[j]=0; queue.append(j)
        if count>1000: found.append((left,top,right+1,bottom+1))
    if len(found)!=16: raise ValueError(f"Expected16 separate whole poses, found {len(found)}")
    found.sort(key=lambda box:(box[1]+box[3])/2)
    return [box for row in range(4) for box in sorted(found[row*4:(row+1)*4],key=lambda box:box[0])]


def assemble(source, output, indices, loop_start, loop_end, repeats, duration):
    image = Image.open(source).convert("RGBA")
    if image.width % 4 or image.height % 4:
        raise ValueError("Source must contain a regular 4 by 4 pose grid")
    width, height = image.width // 4, image.height // 4
    groups=pose_bounds(image)
    if any(l<8 or t<8 or r>image.width-8 or b>image.height-8 for l,t,r,b in groups):
        raise ValueError("Source pose is clipped at image edge")
    source_frames = [image.crop((l-8,t-8,r+8,b+8)) for l,t,r,b in groups]
    bounds = [frame.getchannel("A").getbbox() for frame in source_frames]
    if any(box is None for box in bounds):
        raise ValueError("Empty source frame")
    # Match the accepted neutral pet's height, not independently fit every pose.
    scale = min(198 / (bounds[0][3] - bounds[0][1]), min(176 / (b[2]-b[0]) for b in bounds))
    normalized = []
    for frame, box in zip(source_frames, bounds):
        l,t,r,b = box
        band = frame.getchannel("A").crop((0, b - max(1, (b-t)//4), frame.width, b)).getbbox()
        anchor = (band[0] + band[2]) / 2
        cut = frame.crop(box)
        cut = cut.resize((round(cut.width * scale), round(cut.height * scale)), Image.Resampling.LANCZOS)
        x, y = round(96 - (anchor-l)*scale), 203 - cut.height
        if x < 2 or y < 2 or x+cut.width > 190:
            raise ValueError("Normalized frame would clip; repair source geometry")
        target = Image.new("RGBA", (192,208))
        target.alpha_composite(cut,(x,y))
        normalized.append(target)
    if not indices or any(i < 0 or i >= 16 for i in indices):
        raise ValueError("Invalid frame sequence")
    if not 0 <= loop_start < loop_end <= len(indices):
        raise ValueError("Invalid loop")
    frames = [normalized[i] for i in indices]
    sheet = Image.new("RGBA", (768, ((len(frames)+3)//4)*208))
    contact = Image.new("RGB", (768, sheet.height), "white")
    draw = ImageDraw.Draw(contact)
    for i,frame in enumerate(frames):
        at=(i%4*192, i//4*208)
        sheet.alpha_composite(frame,at)
        contact.paste(frame,at,frame)
        draw.text((at[0]+3, at[1]+3),str(i),fill="black")
    output.parent.mkdir(parents=True,exist_ok=True)
    sheet.save(output,lossless=True)
    contact.save(output.with_suffix(".qa.png"))
    durations=[duration]*len(frames)
    durations[0]=180
    durations[-1]=200
    clip=dict(path=output.name,frameWidth=192,frameHeight=208,columns=4,durationsMs=durations,loopStart=loop_start,loopEnd=loop_end,loopRepeats=repeats)
    output.with_suffix(".json").write_text(json.dumps(clip,indent=2)+"\n")
    order=list(range(loop_start))+list(range(loop_start,loop_end))*repeats+list(range(loop_end,len(frames)))
    previews=[]
    for i in order:
        canvas=Image.new("RGB",(192,208),"white")
        canvas.paste(frames[i],(0,0),frames[i])
        previews.append(canvas)
    previews[0].save(output.with_suffix(".gif"),save_all=True,append_images=previews[1:],duration=[durations[i] for i in order],loop=0,disposal=2)
    report=dict(ok=True,source=str(source),sourceIndices=indices,uniqueSourcePoses=len(set(indices)),scale=scale,frames=len(frames),dimensions=list(sheet.size))
    output.with_suffix(".qa.json").write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps(report))


if __name__ == "__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("source",type=Path)
    parser.add_argument("output",type=Path)
    parser.add_argument("--indices",required=True)
    parser.add_argument("--loop-start",type=int,required=True)
    parser.add_argument("--loop-end",type=int,required=True)
    parser.add_argument("--repeats",type=int,default=3)
    parser.add_argument("--duration",type=int,default=90)
    args=parser.parse_args()
    assemble(args.source,args.output,[int(i) for i in args.indices.split(",")],args.loop_start,args.loop_end,args.repeats,args.duration)
