"""Normalize generated walk poses onto a roomy canvas; never invent limb poses."""
import argparse
import json
from pathlib import Path
from PIL import Image, ImageDraw, ImageChops, ImageStat
from assemble_motion import pose_bounds
from audit_motion import frame_metrics
from export_apng import write_apng

def normalize(source, output):
    source, output = Path(source).resolve(), Path(output).resolve()
    image = Image.open(source).convert("RGBA")
    boxes = pose_bounds(image, 4, 6)
    # One camera scale for the entire cycle, including the tallest tail/ear pose.
    scale = min(192 / max(b[3]-b[1] for b in boxes), 240 / max(b[2]-b[0] for b in boxes))
    frames = []
    for box in boxes:
        l,t,r,b = box
        cut = image.crop((l-4,t-4,r+4,b+4))
        cut = cut.resize((round(cut.width*scale), round(cut.height*scale)), Image.Resampling.LANCZOS)
        solid = frame_metrics(cut)
        x = round(128 - (solid["bounds"][0]+solid["bounds"][2])/2)
        y = 202-solid["baseline"]
        frame = Image.new("RGBA", (256,208)); frame.alpha_composite(cut,(x,y)); frames.append(frame)
        if sum(frame.getchannel("A").histogram()[1:]) != sum(cut.getchannel("A").histogram()[1:]):
            raise ValueError("Visible pose pixels clip at the common camera scale")
    # Register the steady upper head in x only, never move ground-contact paws vertically.
    head_box = (160, 30, 242, 112)
    def opaque(frame):
        bg=Image.new("RGBA",frame.size,(128,128,128,255)); bg.alpha_composite(frame); return bg.convert("RGB")
    reference = opaque(frames[0]).crop(head_box)
    shifts = [0]
    for index in range(1,len(frames)):
        best = None
        for shift in range(-5,6):
            shifted = Image.new("RGBA", (256,208)); shifted.alpha_composite(frames[index],(shift,0))
            if sum(shifted.getchannel("A").histogram()[1:]) != sum(frames[index].getchannel("A").histogram()[1:]): continue
            score=sum(ImageStat.Stat(ImageChops.difference(reference,opaque(shifted).crop(head_box))).mean)
            if best is None or score<best[0]: best=(score,shift,shifted)
        if best: frames[index]=best[2]; shifts.append(best[1])
        else: shifts.append(0)
    output.mkdir(parents=True,exist_ok=True)
    for i,frame in enumerate(frames): frame.save(output/f"frame-{i:02}.png")
    frames[0].save(output/"standing-right.png")
    frames[0].transpose(Image.Transpose.FLIP_LEFT_RIGHT).save(output/"standing-left.png")
    clip=write_apng(output/"cycle.apng",frames,[42]*len(frames))
    contact=Image.new("RGB",(256*6,220*4),"white"); draw=ImageDraw.Draw(contact)
    for i,frame in enumerate(frames):
        at=(i%6*256,i//6*220); contact.paste(frame,at,frame); draw.text((at[0]+4,at[1]+208),str(i),fill="black")
    contact.save(output/"contact.png")
    report={"source":str(source),"scale":scale,"horizontalRegistration":shifts,"clip":clip,"approved":False,
            "notes":"Only translation and uniform scaling; stride and biological loop still require review."}
    (output/"review.json").write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps({"frames":len(frames),"scale":scale,"approved":False}))

if __name__=="__main__":
    parser=argparse.ArgumentParser(); parser.add_argument("source",type=Path); parser.add_argument("output",type=Path)
    args=parser.parse_args(); normalize(args.source,args.output)
