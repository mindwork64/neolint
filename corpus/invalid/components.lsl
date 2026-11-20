default
{
    state_entry()
    {
        vector position;
        float literal = <1.0, 2.0, 3.0>.x;
        position.s = 1.0;
        float wrong = position.s;
        integer count;
        float bad = count.x;
    }
}
