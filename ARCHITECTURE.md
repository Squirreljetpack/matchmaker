## Architecture
### Matchmaker
##### components
- Nucleo
    - worker
    - matcher
- Output function (&Input -> (u32, Output))
- Configs (see config.toml/config.rs)
- Dynamic event/interrupt handlers
- Previewer config

##### pick
```
Event(Bind, Crossterm, Event) -> Event handler -> Action(Context) -> Action -> Computation
    -> Raise Event/Interrupt -> Event/Interrupt Handlers
    -> Render
```