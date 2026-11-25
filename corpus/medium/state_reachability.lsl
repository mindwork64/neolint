// One state is connected to default; one populated state is disconnected.
default
{
    state_entry()
    {
        state active;
    }
}

state active
{
    state_entry()
    {
        state default;
    }
}

state abandoned
{
    state_entry()
    {
        llOwnerSay("No transition reaches this state.");
    }
}
