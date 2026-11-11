# Third-party work

Create reference: Creators-of-Create/Create, commit
`ac0c444d9828da3453ae8cc65338e8de063286fb`, release 6.0.10 (Minecraft 1.21.1).
Copyright (c) The Create Team / The Creators of Create.

The following MIT code informed or was adapted into the Rust engine:

- `content/kinetics/KineticNetwork.java`: capacity and stress scale with absolute RPM.
- `content/kinetics/RotationPropagator.java`: supported coaxial and small/large cog connections.
- `content/kinetics/press/PressingBehaviour.java`: 240-unit cycle, midpoint processing,
  clamped speed interpolation, integer truncation, client receipt/hold/interpolation,
  item-particle request ordering and the rendered head's motion curve.
- `content/processing/basin/BasinOperatingBlockEntity`, `BasinRecipe`,
  `content/kinetics/press/MechanicalPressBlockEntity` and
  `foundation/item/ItemHelper`: shared bounded mixing/compacting recipe compilation,
  square-crafting compression eligibility, deferred Basin checks, completion
  restarts and retained in-flight recipes. World adapters and actual-client
  presentation have separate acceptance evidence and documented limits.
- `content/kinetics/press/BeltPressingCallbacks`, `MechanicalPressBlockEntity`,
  `logistics/depot/DepotBehaviour`, `DepotItemHandler` and `SharedDepotBlockMethods`:
  one-at-a-time default pressing, output buffering, automation and landed items.
- `content/kinetics/simpleRelays/GearboxBlockEntity`, `ClutchBlockEntity`,
  `GearshiftBlockEntity`, `crank/HandCrankBlock[Entity]`, `motor/CreativeMotorBlockEntity`,
  `foundation/block/IWrenchable` and encasing behaviors: implemented transmission,
  placement, wrench, hand crank and motor controls.
- `base/KineticBlockEntityRenderer`, `KineticBlockEntityVisual`, `gearbox/GearboxRenderer`,
  `transmission/SplitShaftRenderer`, `crank/HandCrankRenderer`, `logistics/depot/DepotRenderer`:
  positive world-axis transforms, phase, crank interpolation and item piles.
- `gauge/GaugeBlock[Entity]`, `SpeedGaugeBlockEntity`, `StressGaugeBlockEntity`,
  `GaugeRenderer`, `base/DirectionalAxisKineticBlock`, `gearbox/VerticalGearboxItem`,
  `equipment/goggles/GogglesItem`, `GogglesModel`: placement, instrument targets,
  comparator output, equipment and display contexts.
- `waterwheel/WaterWheelBlock[Entity]`, `LargeWaterWheelBlock[Entity]`,
  `LargeWaterWheelBlockItem`, `WaterWheelStructuralBlock`, `WaterWheelRenderer`:
  flow projection/scoring, generated speed, capacity, materials and structural occupancy.
- `content/kinetics/millstone/MillstoneBlock`, `MillstoneBlockEntity`,
  `MillstoneRenderer`, `MillingRecipe`, and `AllShapes`: fixed-axis/cog connections,
  collision, filtered input/output inventories, timer, collection and moving part.
- `content/kinetics/fan/EncasedFanBlock`, `EncasedFanBlockEntity`,
  `IAirCurrentSource`, `AirCurrent`, `EncasedFanRenderer`, `FanVisual`,
  `fan/processing/FanProcessing`, `AllFanProcessingTypes`, `SplashingRecipe` and
  `HauntingRecipe`: placement and shaft direction, airflow range/segments,
  obstruction sampling, dropped/depot processing timers, catalyst rules,
  furnace recipe selection and propeller speed.
- `content/processing/recipe/ProcessingOutput`: per-output-item probability rolls
  and result construction. `content/kinetics/belt/transport/TransportedItemStack` supplies the
  transported fan-processing field conventions.
- `content/kinetics/drill/DrillBlock`, `DrillBlockEntity`, `DrillRenderer`, and
  `content/kinetics/base/BlockBreakingKineticBlockEntity`: stationary front-block
  selection, break progress/retry timing, source NBT names, ordinary loot, rear
  shaft and rotating head. Contraption movement and entity damage are not ported.
- `content/processing/burner/BlazeBurnerBlock`, `BlazeBurnerBlockEntity`,
  `BlazeBurnerBlockItem`, `LitBlazeBurnerBlock` and `api/registry/CreateDataMaps`:
  source state/factory identities, fuel precedence and transitions, consumption
  and remainder rules, heat/comparator policy and canonical saved fields.
  Native integration and presentation are validated separately from the pure
  fuel oracle; incomplete capture/automation/contraption paths are not implied.
- `AllSoundEvents` and infrastructure configs: selected composite sound events,
  default gains/pitches, stress and speed parameters.
- `content/equipment/goggles/GoggleOverlayRenderer`,
  `foundation/gui/RemovedGuiUtils`, `foundation/item/TooltipHelper`,
  `content/kinetics/base/IRotate`, `KineticBlockEntity`,
  `GeneratingKineticBlockEntity`, `foundation/blockEntity/behaviour/ValueSettingsScreen`,
  `ValueSettingsClient`, `ValueSettingsBoard`, `ValueBox`, `ValueBoxTransform`,
  `scrollValue/ScrollValueRenderer` and `motor/KineticScrollValueBehaviour`:
  original tooltip providers, text styles, thresholds, layout, hover fade and
  value-board geometry and interaction ordering, motor world value display poses.
- `content/kinetics/base/KineticEffectHandler`, `RotationIndicatorParticle`,
  `RotationIndicatorParticleData`, `content/kinetics/fan/AirFlowParticle` and
  `AirFlowParticleData`: rotation indicators and fan particle motion, appearance
  and processing-type transformations.
- `content/schematics/SchematicItem`, `SchematicPrinter`,
  `packet/SchematicPlacePacket`, and `foundation/utility/BlockHelper`:
  shared fixture state/NBT conventions and original creative-placement policy.
  The development Java bridge calls the original placement packet directly;
  the Rust schematic adapter is independently implemented and validated.
- Selected MIT datapack recipes, behavior/ingredient tags and fuel data maps, exported from
  `src/generated/resources/data/` at the pinned revision. Ingredient syntax is
  adapted to Horizon/Minecraft 26.3. Missing vanilla convention tags are our own
  definitions and supplement Create's tag additions with vanilla entries.
  Fan block tags omit unimplemented Create blocks. Recipes requiring unavailable
  items or other mods are excluded.

Paths above are under `src/main/java/com/simibubi/create/`, except the recipe/tag data under
`src/generated/resources/data/`. The MIT notice is in LICENSE.
Version 0.5 remains incomplete and does not claim full behavioral parity.
Validation evidence and its limits are recorded in `VALIDATION.md`.
Rust code organization is native to Horizon; source behavior is the reference,
not one Rust type for every Java class.

Ponder **1.0.82+mc1.21.1**, bundled in the player's pinned Create JAR, contains
Catnip's `net.createmod.catnip.lang.LangBuilder`, `LangNumberFormat`,
`FontHelper`, `animation.LerpedFloat` (linear/EXP tank levels),
`gui.element.BoxElement`, `outliner.Outliner`, `math.AngleHelper` and `theme.Color`. Their MIT code informs
tooltip indentation, locale number formatting, highlighting, default colors and
alpha behavior, world display expiry, belt/motor orientation and fluid-level
chase/interpolation. The bundled JAR records commit
`355b488f3d5c5756b33de190d2dd4f7e483480e0` in its `Git-Hash` manifest field.
The belt render oracle calls this original `AngleHelper` class from the private
reference classpath; public Java/Rust recording templates retain MIT attribution
and contain no original models or textures. The exact notice below is from the
bundled JAR's `LICENSE_Ponder`.
This attribution concerns code; it does not change the resource license described
below.

```text
MIT License

Copyright (c) 2022 The Create Team

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

Create's original `src/main/resources/assets/` has a separate All Rights Reserved
license. Development inputs and port-pack carry-over stay under `.local/`.
The optional offline importer reads a JAR supplied by the player in their instance's
`horizon-mods/create-resources/`, then caches the required resources under that
instance's `.horizon-cache/create-assets/`. It does not download or execute the JAR,
modify the installed mod or grant rights to redistribute the imported resources.
The input JAR and generated caches are excluded from Git and all release archives.
Local extraction is not a guarantee of legal clearance. See `ASSETS.md`. The default
content uses our provisional JSON models and refers to vanilla textures by ID;
it embeds no Mojang textures or original Create textures.

The resource bootstrap is the published Create **6.0.10** release `UjX6dr61` on
Modrinth. Its provenance and verified hashes are in `.local/import-provenance.json`.
Its source, behavior oracle and resource baseline now match the same published
6.0.10 release. Every locale is imported from the local JAR without redistribution.

Additional MIT references used in the placement/depot adapter:
- `RotatedPillarKineticBlock`, `DirectionalKineticBlock`, `CreativeMotorBlock`,
  `CogWheelBlock`: placement choice and neighboring shaft ports.
- `SharedDepotBlockMethods.onUse`, `DepotBehaviour`, `TransportedItemStack`:
  top-face exchange and saved field names. Java method bodies used by the oracle
  are AST-exported into `.local` and are not shipped as player code.
- Horizon `horizon-world/src/level/place.rs` (MIT, horizon-rust contributors):
  the nearest-looking-direction calculation, adapted to the placement adapter.

Horizon and Horizon Mixins are MIT. Their original checkouts are not edited or
included in the mod. Builds pin the published commit
`e8cd0aa38cffef49cb2e088ef716baa107148c02` of Horizon Mixins, accepted through
[upstream PR #25](https://github.com/horizon-rust/horizon-mixins/pull/25), which
recognizes Rust's built-in `recursion_limit` attribute. The launcher fetches that
commit from the official repository; no tool source is patched locally. This is
a pinned source dependency, not an upstream approval of this port.
