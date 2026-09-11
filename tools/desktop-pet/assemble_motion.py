"""Assemble already-matted AI pose grids; preserves one scale and ground anchor.

No image API calls or generated in-betweens. Reordering is explicit in the report.
Requires Pillow. Output is an Astro motion clip plus a contact sheet and GIF.
"""
import argparse
import json
from collections import deque
from pathlib import Path
from PIL import Image, ImageDraw


def pose_bounds(image, columns=4, rows=4):
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
    expected = columns * rows
    if len(found)!=expected: raise ValueError(f"Expected {expected} separate whole poses, found {len(found)}")
    found.sort(key=lambda box:(box[1]+box[3])/2)
    return [box for row in range(rows) for box in sorted(found[row*columns:(row+1)*columns],key=lambda box:box[0])]


def assemble(source, output, indices, loop_start, loop_end, repeats, duration,
             source_columns=4, source_rows=4, neutral_cell=None, holds=None, anchor_mode="feet", safe_margin=2):
    image = Image.open(source).convert("RGBA")
    if source_columns < 1 or source_rows < 1:
        raise ValueError("Source grid dimensions must be positive")
    if not indices or any(i < 0 or i >= source_columns*source_rows for i in indices):
        raise ValueError("Invalid frame sequence")
    groups=pose_bounds(image, source_columns, source_rows)
    if any(l<8 or t<8 or r>image.width-8 or b>image.height-8 for l,t,r,b in groups):
        raise ValueError("Source pose is clipped at image edge")
    source_frames = [image.crop((l-8,t-8,r+8,b+8)) for l,t,r,b in groups]
    bounds = [frame.getchannel("A").getbbox() for frame in source_frames]
    if any(box is None for box in bounds):
        raise ValueError("Empty source frame")
    # Match the accepted neutral pet's height, not independently fit every pose.
    scale = min(198 / (bounds[0][3] - bounds[0][1]), min(176 / (b[2]-b[0]) for b in bounds))
    neutral = Image.open(neutral_cell).convert("RGBA") if neutral_cell else None
    baseline, target_anchor = 203, 96
    if neutral is not None:
        if neutral.size != (192,208): raise ValueError("Neutral cell must be 192x208")
        box = neutral.getchannel("A").getbbox()
        if box is None: raise ValueError("Neutral cell is empty")
        l,t,r,b = box
        band = neutral.getchannel("A").crop((0,b-max(1,(b-t)//4),192,b)).getbbox()
        baseline, target_anchor = b, (band[0]+band[2])/2
        if anchor_mode == "body": target_anchor = (l+r)/2
        # Never silently shrink the pet to fit a wide pose. Repair the source
        # if it cannot fit at the canonical size; this avoids entry/exit pops.
        scale = (b-t) / (bounds[0][3]-bounds[0][1])
    normalized = []
    used_source_indices = set(indices[1:-1] if neutral is not None else indices)
    for frame_index, (frame, box) in enumerate(zip(source_frames, bounds)):
        if frame_index not in used_source_indices:
            normalized.append(None)
            continue
        l,t,r,b = box
        band = frame.getchannel("A").crop((0, b - max(1, (b-t)//4), frame.width, b)).getbbox()
        anchor = (band[0] + band[2]) / 2
        if anchor_mode == "body": anchor = (l+r)/2
        cut = frame.crop(box)
        cut = cut.resize((round(cut.width * scale), round(cut.height * scale)), Image.Resampling.LANCZOS)
        x, y = round(target_anchor - (anchor-l)*scale), baseline - cut.height
        if x < safe_margin or y < safe_margin or x+cut.width > 192-safe_margin or y+cut.height > 208-safe_margin:
            raise ValueError(f"Normalized frame {frame_index} would clip: x={x}, y={y}, size={cut.size}; repair source geometry")
        target = Image.new("RGBA", (192,208))
        target.alpha_composite(cut,(x,y))
        normalized.append(target)
    if not indices or any(i < 0 or i >= source_columns*source_rows for i in indices):
        raise ValueError("Invalid frame sequence")
    if not 0 <= loop_start < loop_end <= len(indices):
        raise ValueError("Invalid loop")
    frames = [normalized[i] for i in indices]
    if neutral is not None:
        frames[0] = neutral.copy()
        frames[-1] = neutral.copy()
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
    for at, milliseconds in (holds or {}).items():
        if not 0 <= at < len(frames) or not 20 <= milliseconds <= 2000:
            raise ValueError("Invalid hold frame or duration")
        durations[at] = milliseconds
    clip=dict(path=output.name,frameWidth=192,frameHeight=208,columns=4,durationsMs=durations,loopStart=loop_start,loopEnd=loop_end,loopRepeats=repeats)
    if neutral is not None: clip["neutralBookends"] = True
    output.with_suffix(".json").write_text(json.dumps(clip,indent=2)+"\n")
    order=list(range(loop_start))+list(range(loop_start,loop_end))*repeats+list(range(loop_end,len(frames)))
    previews=[]
    for i in order:
        canvas=Image.new("RGB",(192,208),"white")
        canvas.paste(frames[i],(0,0),frames[i])
        previews.append(canvas)
    previews[0].save(output.with_suffix(".gif"),save_all=True,append_images=previews[1:],duration=[durations[i] for i in order],loop=0,disposal=2)
    previews[0].save(output.with_suffix(".slow.gif"),save_all=True,append_images=previews[1:],duration=[durations[i]*3 for i in order],loop=0,disposal=2)
    for background in ("black", "checker"):
        qa=Image.new("RGB",sheet.size,"black")
        if background == "checker":
            draw=ImageDraw.Draw(qa)
            for y in range(0,qa.height,12):
                for x in range(0,qa.width,12): draw.rectangle((x,y,x+11,y+11),fill="#eeeeee" if (x//12+y//12)%2 else "#bbbbbb")
        qa.paste(sheet,(0,0),sheet)
        qa.save(output.with_suffix(f".{background}.png"))
    used = indices[1:-1] if neutral is not None else indices
    report=dict(ok=True,source=str(source),sourceIndices=indices,uniqueSourcePoses=len(set(used)),generatedPoseCount=source_columns*source_rows,neutralReplacements=[0,len(frames)-1] if neutral is not None else [],anchorMode=anchor_mode,safeMargin=safe_margin,scale=scale,frames=len(frames),dimensions=list(sheet.size))
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
    parser.add_argument("--source-columns",type=int,default=4)
    parser.add_argument("--source-rows",type=int,default=4)
    parser.add_argument("--neutral-cell",type=Path)
    parser.add_argument("--hold",action="append",default=[],help="Playback index:milliseconds")
    parser.add_argument("--anchor",choices=["feet","body"],default="feet")
    parser.add_argument("--safe-margin",type=int,choices=range(1,9),default=2)
    args=parser.parse_args()
    holds = dict(tuple(int(v) for v in item.split(":")) for item in args.hold)
    assemble(args.source,args.output,[int(i) for i in args.indices.split(",")],args.loop_start,args.loop_end,args.repeats,args.duration,args.source_columns,args.source_rows,args.neutral_cell,holds,args.anchor,args.safe_margin)
