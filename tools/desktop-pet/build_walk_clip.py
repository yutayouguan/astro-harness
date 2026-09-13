"""Assemble an authored stand/turn transition and a registered walk cycle.

Exact neutral endpoints are retained. Reversing the grounded stand transition
provides a sit-down/turn-back, never a reversed walk cycle.
"""
import argparse
import json
from pathlib import Path
from PIL import Image, ImageDraw
from assemble_motion import pose_bounds
from audit_motion import frame_metrics
from export_apng import write_apng

def build(transition, cycle_dir, direction, output):
    image = Image.open(transition).convert("RGBA")
    boxes = pose_bounds(image, 4, 3)
    neutral = Image.open(cycle_dir / "neutral.png").convert("RGBA")
    target = Image.open(cycle_dir / f"standing-{direction}.png").convert("RGBA")
    start, end = frame_metrics(neutral), frame_metrics(target)
    scale = (start["bounds"][3]-start["bounds"][1]) / (boxes[0][3]-boxes[0][1])
    mids, offsets = [], []
    for index, (l,t,r,b) in enumerate(boxes[1:-1], 1):
        cut = image.crop((l-4,t-4,r+4,b+4))
        cut = cut.resize((round(cut.width*scale),round(cut.height*scale)),Image.Resampling.LANCZOS)
        info = frame_metrics(cut)
        anchor = start["footprintCenter"] + (end["footprintCenter"]-start["footprintCenter"]) * index / 11
        x = round(anchor-info["footprintCenter"])
        if cut.width > 252: raise ValueError(f"Transition pose {index} exceeds safe canvas width")
        safe_x = max(2,min(254-cut.width,x))
        y = start["baseline"]-info["baseline"]
        frame = Image.new("RGBA",(256,208)); frame.alpha_composite(cut,(safe_x,y))
        if sum(frame.getchannel("A").histogram()[1:]) != sum(cut.getchannel("A").histogram()[1:]):
            raise ValueError(f"Transition pose {index} clips visible pixels")
        offsets.append(safe_x-x); mids.append(frame)
    cycle = [Image.open(path).convert("RGBA") for path in sorted(cycle_dir.glob("frame-*.png"))]
    if direction == "left": cycle = [frame.transpose(Image.Transpose.FLIP_LEFT_RIGHT) for frame in cycle]
    if len(cycle) != 24: raise ValueError("Expected a 24-pose walking cycle")
    frames = [neutral] + mids + cycle + list(reversed(mids)) + [neutral]
    start_index, end_index = 1+len(mids), 1+len(mids)+len(cycle)
    times = [42]*len(frames); times[0]=180; times[-1]=200
    output.mkdir(parents=True,exist_ok=True)
    spec=write_apng(output/f"running-{direction}.apng",frames,times,start_index,end_index,3)
    spec["locomotion"]={"stridePx":64}
    (output/f"running-{direction}.json").write_text(json.dumps(spec,indent=2)+"\n")
    sheet=Image.new("RGB",(256*6,220*((len(frames)+5)//6)),"white"); draw=ImageDraw.Draw(sheet)
    for i,frame in enumerate(frames):
        at=(i%6*256,i//6*220);sheet.paste(frame,at,frame);draw.text((at[0]+3,at[1]+208),str(i),fill="black")
    sheet.save(output/f"running-{direction}-contact.png")
    (output/f"running-{direction}-review.json").write_text(json.dumps({"approved":False,"transition":str(transition),"scale":scale,"edgeOffsets":offsets,"loop":[start_index,end_index],"frames":len(frames)},indent=2)+"\n")
    print(json.dumps({"frames":len(frames),"scale":scale,"edgeOffsets":offsets,"approved":False}))

if __name__=="__main__":
    parser=argparse.ArgumentParser();parser.add_argument("transition",type=Path);parser.add_argument("cycle_dir",type=Path);parser.add_argument("direction",choices=["left","right"]);parser.add_argument("output",type=Path)
    args=parser.parse_args();build(args.transition,args.cycle_dir,args.direction,args.output)
