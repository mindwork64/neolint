list left = [1, 2];
list right = [3, 4];

default
{
    state_entry()
    {
        if (left == right)
            llOwnerSay("same length");
        if (left != [])
            llOwnerSay("different length");
    }
}
