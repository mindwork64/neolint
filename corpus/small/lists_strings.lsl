list gNames = ["alpha", "beta", "gamma"];
key gOwner;

default
{
    state_entry()
    {
        gOwner = llGetOwner();
        integer count = llGetListLength(gNames);
        integer i;
        for (i = 0; i < count; i++)
        {
            string name = llList2String(gNames, i);
            llOwnerSay(name + ":" + (string)gOwner);
        }

        list parts = llParseString2List("one,two,three", [","], []);
        gNames = gNames + parts;
        llOwnerSay(llList2String(gNames, 3));
    }
}
