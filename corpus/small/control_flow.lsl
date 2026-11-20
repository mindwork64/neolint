integer gMode = 0;

integer classify(integer value)
{
    integer result = 0;
    if (value < 0) result = -1;
    else if (value == 0) result = 0;
    else result = 1;
    return result;
}

default
{
    state_entry()
    {
        integer i = 0;
        while (i < 3) i++;
        do { i--; } while (i > 0);
        jump done;
        @done;
        llOwnerSay((string)classify(gMode));
    }
}
