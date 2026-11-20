default
{
    state_entry()
    {
        state running;
    }
}

state running
{
    state_entry()
    {
        llOwnerSay("first");
    }
}

state running
{
    state_entry()
    {
        llOwnerSay("second");
    }
}
