import {Composition} from "remotion";
import {WindowSemantics} from "./WindowSemantics";

export const Root = () => (
  <Composition
    id="WindowSemantics"
    component={WindowSemantics}
    durationInFrames={240}
    fps={30}
    width={1200}
    height={600}
  />
);
