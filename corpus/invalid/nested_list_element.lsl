integer duplicate;
integer duplicate;
list child = [1];
default
{
    state_entry()
    {
        list outer = [child];
        llOwnerSay((string)outer);
    }
}
