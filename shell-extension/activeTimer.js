// D-Bus contract with the Houra application. ACTIVE_TIMER_XML must stay
// identical to data/dbus/io.github.majamato.Houra.ActiveTimer.xml; a Rust
// test in crates/app/tests/top_bar_contract.rs enforces this.

export const APP_ID = 'io.github.majamato.Houra';
export const BUS_NAME = APP_ID;
export const OBJECT_PATH = `/${APP_ID.replaceAll('.', '/')}`;

export const ACTIVE_TIMER_XML = `<node>
  <!-- Houra's active timer, shown by Houra's GNOME Shell extension. -->
  <interface name="io.github.majamato.Houra.ActiveTimer">
    <!-- Pauses a running timer or resumes a paused one. -->
    <method name="TogglePause"/>
    <!-- Opens the pending idle or recovery review in the Houra window. -->
    <method name="OpenReview"/>
    <!-- stopped, running, paused, idle or recovery. -->
    <property name="State" type="s" access="read"/>
    <!-- The active time entry's total in milliseconds when this value was published. -->
    <property name="ElapsedMs" type="t" access="read"/>
    <!-- "note · project · activity" for accessible names; empty when stopped. -->
    <property name="Summary" type="s" access="read"/>
  </interface>
</node>`;
