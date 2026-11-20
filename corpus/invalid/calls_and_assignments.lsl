integer broken()
{
    return "not an integer";
}

default
{
    state_entry()
    {
        llSetPos("not a vector");
        llSleep("slow");
        integer value;
        value = "text";
        llDialog(llGetOwner(), "ok", ["one"], "channel");
    }
}
