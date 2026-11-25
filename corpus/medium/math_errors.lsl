default
{
    state_entry()
    {
        integer division = 10 / 0;
        integer remainder = 7 % (3 - 3);
        float negative_zero = 1.0 / -0.0;
        integer compound = 8;
        compound /= FALSE;

        llOwnerSay((string)division);
        llOwnerSay((string)remainder);
        llOwnerSay((string)negative_zero);
        llOwnerSay((string)compound);
    }
}
